use crate::easyeda::models::{ApiResponse, ComponentData, Model3dInfo, SymbolPartData};
use crate::error::{AppError, EasyedaError, Result};
use futures_util::StreamExt;
use reqwest::header::ACCEPT_ENCODING;
use reqwest::{Client, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OnceCell};

const MAX_REQUEST_ATTEMPTS: u32 = 4;
const RETRY_BASE_DELAY_MS: u64 = 250;
const STEP_DOWNLOAD_ATTEMPTS: u32 = 2;
const STEP_DOWNLOAD_TOTAL_TIMEOUT_SECS: u64 = 60;
const STEP_DOWNLOAD_STALL_TIMEOUT_SECS: u64 = 20;
const UPSTREAM_BLOCK_RETRY_DELAY_MS: u64 = 10_000;
const UPSTREAM_BLOCK_COOLDOWN_MS: u64 = 45_000;
const COMPONENT_REQUEST_INTERVAL_MS: u64 = 750;
const MODEL_REQUEST_INTERVAL_MS: u64 = 500;
type DownloadCell = Arc<OnceCell<std::result::Result<Arc<Vec<u8>>, String>>>;
type DownloadCache = Arc<Mutex<HashMap<String, DownloadCell>>>;

pub struct EasyedaApi {
    client: Client,
    model_client: Client,
    step_cache: DownloadCache,
    component_request_lock: Arc<Mutex<()>>,
    model_download_lock: Arc<Mutex<()>>,
    component_gate: Arc<Mutex<RequestGate>>,
    model_gate: Arc<Mutex<RequestGate>>,
}

#[derive(Debug)]
struct RequestGate {
    next_ready: Instant,
}

impl RequestGate {
    fn new() -> Self {
        Self {
            next_ready: Instant::now(),
        }
    }
}

impl EasyedaApi {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent(format!("npnp/{}", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(5))
                .read_timeout(std::time::Duration::from_secs(30))
                .pool_max_idle_per_host(10)
                .http2_adaptive_window(true)
                .build()
                .expect("Failed to create HTTP client"),
            model_client: Client::builder()
                .user_agent(format!("npnp/{}", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(5))
                .read_timeout(std::time::Duration::from_secs(30))
                .pool_max_idle_per_host(32)
                .http1_only()
                .build()
                .expect("Failed to create model HTTP client"),
            step_cache: Arc::new(Mutex::new(HashMap::new())),
            component_request_lock: Arc::new(Mutex::new(())),
            model_download_lock: Arc::new(Mutex::new(())),
            component_gate: Arc::new(Mutex::new(RequestGate::new())),
            model_gate: Arc::new(Mutex::new(RequestGate::new())),
        }
    }

    pub async fn get_component_data(&self, lcsc_id: &str) -> Result<ComponentData> {
        let mut last_error = None;

        for attempt in 0..MAX_REQUEST_ATTEMPTS {
            match self.get_component_data_once(lcsc_id).await {
                Ok(data) => return Ok(data),
                Err(e) if attempt + 1 < MAX_REQUEST_ATTEMPTS && should_retry_app_error(&e) => {
                    log::debug!(
                        "Failed to fetch component data for {} (retry {}/{}): {}",
                        lcsc_id,
                        attempt + 1,
                        MAX_REQUEST_ATTEMPTS,
                        e
                    );
                    let delay = retry_delay_for_error(&e, attempt);
                    last_error = Some(e);
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_error.expect("retry loop should preserve the last error"))
    }

    async fn get_component_data_once(&self, lcsc_id: &str) -> Result<ComponentData> {
        let _request_lock = self.component_request_lock.lock().await;
        wait_for_request_slot(
            &self.component_gate,
            Duration::from_millis(COMPONENT_REQUEST_INTERVAL_MS),
        )
        .await;

        let url = format!(
            "https://easyeda.com/api/products/{}/components?version=6.4.19.5",
            lcsc_id
        );

        log::debug!("Fetching component data for {}", lcsc_id);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(EasyedaError::ApiRequest)?;

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_default()
                .trim()
                .chars()
                .take(200)
                .collect::<String>();
            if status == StatusCode::NOT_FOUND {
                return Err(EasyedaError::ComponentNotFound(lcsc_id.to_string()).into());
            }
            let message = summarize_http_error_body(message);
            if is_upstream_block_status(status, &message) {
                extend_request_cooldown(
                    &self.component_gate,
                    Duration::from_millis(UPSTREAM_BLOCK_COOLDOWN_MS),
                )
                .await;
            }
            return Err(EasyedaError::HttpStatus {
                lcsc_id: lcsc_id.to_string(),
                status,
                message,
            }
            .into());
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .map_err(|e| EasyedaError::InvalidData(format!("Failed to parse JSON: {}", e)))?;

        if !api_response.success {
            let message = api_response_message(&api_response);
            if is_not_found_api_message(&message) {
                return Err(EasyedaError::ComponentNotFound(lcsc_id.to_string()).into());
            }
            return Err(EasyedaError::ApiResponse {
                lcsc_id: lcsc_id.to_string(),
                message,
            }
            .into());
        }

        let result = api_response
            .result
            .ok_or_else(|| EasyedaError::InvalidData("Missing result field".to_string()))?;

        let data_str_obj = result
            .data_str
            .as_ref()
            .ok_or_else(|| EasyedaError::InvalidData("Missing dataStr field".to_string()))?;

        log::debug!("data_str_obj type: {:?}", data_str_obj);

        let bbox_x = data_str_obj
            .get("head")
            .and_then(|h| h.get("x"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let bbox_y = data_str_obj
            .get("head")
            .and_then(|h| h.get("y"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        log::debug!("Extracted bbox: x={}, y={}", bbox_x, bbox_y);

        let data_str =
            if let Some(shape_array) = data_str_obj.get("shape").and_then(|v| v.as_array()) {
                log::debug!("Found shape array with {} elements", shape_array.len());
                shape_array
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                log::warn!("data_str_obj doesn't have shape array");
                vec![]
            };

        log::debug!("Final data_str has {} shapes", data_str.len());

        log::debug!("API result has {} raw subparts", result.subparts.len());

        let symbol_parts = result
            .subparts
            .iter()
            .filter_map(|subpart| {
                let data_str_obj = subpart.data_str.as_ref()?;
                let shapes = extract_shape_strings(data_str_obj);
                if shapes.is_empty() {
                    return None;
                }

                Some(SymbolPartData {
                    title: subpart.title.clone().unwrap_or_default(),
                    data_str: shapes,
                    bbox_x: symbol_bbox_x(data_str_obj),
                    bbox_y: symbol_bbox_y(data_str_obj),
                })
            })
            .collect::<Vec<_>>();

        if !symbol_parts.is_empty() {
            log::debug!("Found {} symbol subparts", symbol_parts.len());
        }

        let title = result
            .title
            .ok_or_else(|| EasyedaError::InvalidData("Missing title field".to_string()))?;

        let designator = data_str_obj
            .get("head")
            .and_then(|head| {
                head.get("c_para")
                    .and_then(|params| params.get("pre"))
                    .and_then(|value| value.as_str())
                    .or_else(|| head.get("pre").and_then(|value| value.as_str()))
            })
            .unwrap_or_default()
            .trim()
            .to_string();

        let manufacturer = data_str_obj
            .get("head")
            .and_then(|h| h.get("c_para"))
            .and_then(|cp| cp.get("BOM_Manufacturer"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let package = data_str_obj
            .get("head")
            .and_then(|h| h.get("c_para"))
            .and_then(|cp| cp.get("package"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let szlcsc_id = result
            .lcsc
            .as_ref()
            .and_then(|lcsc| lcsc.get("id"))
            .and_then(|v| v.as_u64());

        let datasheet = result
            .lcsc
            .as_ref()
            .and_then(|lcsc| lcsc.get("url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if let Some(id) = szlcsc_id {
                    format!("https://item.szlcsc.com/datasheet/{}/{}.html", title, id)
                } else {
                    String::new()
                }
            });

        let jlc_id = data_str_obj
            .get("head")
            .and_then(|h| h.get("c_para"))
            .and_then(|cp| cp.get("BOM_JLCPCB Part Class"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Build description: use API description if available, otherwise generate from metadata
        let description = match result.description {
            Some(ref d) if !d.is_empty() => d.clone(),
            _ => {
                let mut parts = Vec::new();
                if !manufacturer.is_empty() {
                    parts.push(manufacturer.clone());
                }
                if !title.is_empty() {
                    parts.push(title.clone());
                }
                if !package.is_empty() {
                    parts.push(package.clone());
                }
                parts.join(" ")
            }
        };

        log::debug!(
            "Extracted metadata: manufacturer={}, datasheet={}, jlc_id={}, description={}",
            manufacturer,
            datasheet,
            jlc_id,
            description
        );

        let (package_detail, package_bbox_x, package_bbox_y, model_3d) = if let Some(pkg) =
            result.package_detail
        {
            let pkg_bbox_x = pkg
                .get("dataStr")
                .and_then(|ds| ds.get("head"))
                .and_then(|h| h.get("x"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let pkg_bbox_y = pkg
                .get("dataStr")
                .and_then(|ds| ds.get("head"))
                .and_then(|h| h.get("y"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);

            log::debug!("Extracted package bbox: x={}, y={}", pkg_bbox_x, pkg_bbox_y);

            let shapes = if let Some(pkg_data_str) = pkg.get("dataStr") {
                if let Some(shape_array) = pkg_data_str.get("shape").and_then(|v| v.as_array()) {
                    shape_array
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                } else {
                    vec![]
                }
            } else if pkg.is_array() {
                pkg.as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                vec![]
            };

            let model_3d = Self::extract_3d_model_from_svgnode(&shapes);
            (shapes, pkg_bbox_x, pkg_bbox_y, model_3d)
        } else {
            (vec![], 0.0, 0.0, None)
        };

        Ok(ComponentData {
            lcsc_id: lcsc_id.to_string(),
            title,
            designator,
            description,
            data_str,
            bbox_x,
            bbox_y,
            symbol_parts,
            package_detail,
            package_bbox_x,
            package_bbox_y,
            model_3d,
            manufacturer,
            datasheet,
            jlc_id,
        })
    }

    fn extract_3d_model_from_svgnode(shapes: &[String]) -> Option<Model3dInfo> {
        for shape in shapes {
            if shape.starts_with("SVGNODE~") {
                let parts: Vec<&str> = shape.split('~').collect();
                if parts.len() > 1
                    && let Ok(svg_data) = serde_json::from_str::<serde_json::Value>(parts[1])
                    && let Some(attrs) = svg_data.get("attrs")
                    && attrs.get("c_etype").and_then(|v| v.as_str()) == Some("outline3D")
                {
                    let uuid = attrs
                        .get("uuid")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let title = attrs
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    if let (Some(uuid), Some(title)) = (uuid, title) {
                        return Some(Model3dInfo { uuid, title });
                    }
                }
            }
        }
        None
    }

    pub async fn download_3d_step(&self, uuid: &str) -> Result<Vec<u8>> {
        let url = format!(
            "https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{}",
            uuid
        );
        let cell = {
            let mut cache = self.step_cache.lock().await;
            cache
                .entry(uuid.to_string())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        let result = cell
            .get_or_init(|| async {
                self.download_with_retry(&url, "STEP", uuid)
                    .await
                    .map(Arc::new)
                    .map_err(|e| e.to_string())
            })
            .await
            .clone();

        match result {
            Ok(bytes) => Ok(bytes.as_ref().clone()),
            Err(message) => {
                self.step_cache.lock().await.remove(uuid);
                Err(EasyedaError::InvalidData(message).into())
            }
        }
    }

    async fn download_with_retry(
        &self,
        url: &str,
        model_type: &str,
        uuid: &str,
    ) -> Result<Vec<u8>> {
        let _download_lock = self.model_download_lock.lock().await;
        for attempt in 1..=STEP_DOWNLOAD_ATTEMPTS {
            log::info!(
                "Downloading 3D {} model: {}{}",
                model_type,
                uuid,
                if attempt > 1 {
                    format!(" (retry {}/{})", attempt, STEP_DOWNLOAD_ATTEMPTS)
                } else {
                    String::new()
                }
            );

            let result = tokio::time::timeout(
                Duration::from_secs(STEP_DOWNLOAD_TOTAL_TIMEOUT_SECS),
                self.download_once(url, model_type, uuid),
            )
            .await;

            match result {
                Ok(Ok(bytes)) => return Ok(bytes),
                Ok(Err(error)) => {
                    if attempt == STEP_DOWNLOAD_ATTEMPTS {
                        return Err(error);
                    }
                    log::warn!("Failed to download {} model, retrying...", model_type);
                    Self::backoff_delay(attempt).await;
                }
                Err(_) => {
                    if attempt == STEP_DOWNLOAD_ATTEMPTS {
                        return Err(EasyedaError::InvalidData(format!(
                            "{} download timed out after {}s: {}",
                            model_type, STEP_DOWNLOAD_TOTAL_TIMEOUT_SECS, uuid
                        ))
                        .into());
                    }
                    log::warn!("{} download timed out, retrying...", model_type);
                    Self::backoff_delay(attempt).await;
                }
            }
        }
        unreachable!()
    }

    async fn download_once(&self, url: &str, model_type: &str, uuid: &str) -> Result<Vec<u8>> {
        wait_for_request_slot(
            &self.model_gate,
            Duration::from_millis(MODEL_REQUEST_INTERVAL_MS),
        )
        .await;

        let response = self
            .model_client
            .get(url)
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(EasyedaError::ApiRequest)?;

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_default()
                .trim()
                .chars()
                .take(200)
                .collect::<String>();
            let message = summarize_http_error_body(message);
            if is_upstream_block_status(status, &message) {
                extend_request_cooldown(
                    &self.model_gate,
                    Duration::from_millis(UPSTREAM_BLOCK_COOLDOWN_MS),
                )
                .await;
            }
            return Err(EasyedaError::HttpStatus {
                lcsc_id: uuid.to_string(),
                status,
                message: format!("Failed to download {model_type}: {message}"),
            }
            .into());
        }

        read_response_body(response, STEP_DOWNLOAD_STALL_TIMEOUT_SECS)
            .await
            .map_err(EasyedaError::InvalidData)
            .map_err(Into::into)
    }

    async fn backoff_delay(attempt: u32) {
        tokio::time::sleep(retry_delay_for_attempt(attempt)).await;
    }
}

fn symbol_bbox_x(data_str_obj: &serde_json::Value) -> f64 {
    data_str_obj
        .get("head")
        .and_then(|h| h.get("x"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn symbol_bbox_y(data_str_obj: &serde_json::Value) -> f64 {
    data_str_obj
        .get("head")
        .and_then(|h| h.get("y"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn extract_shape_strings(data_str_obj: &serde_json::Value) -> Vec<String> {
    data_str_obj
        .get("shape")
        .and_then(|v| v.as_array())
        .map(|shape_array| {
            shape_array
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

async fn wait_for_request_slot(gate: &Mutex<RequestGate>, interval: Duration) {
    loop {
        let wait = {
            let mut gate = gate.lock().await;
            let now = Instant::now();
            if gate.next_ready <= now {
                gate.next_ready = now + interval;
                None
            } else {
                Some(gate.next_ready.saturating_duration_since(now))
            }
        };

        let Some(wait) = wait else {
            return;
        };
        tokio::time::sleep(wait).await;
    }
}

async fn extend_request_cooldown(gate: &Mutex<RequestGate>, cooldown: Duration) {
    let mut gate = gate.lock().await;
    let until = Instant::now() + cooldown;
    if gate.next_ready < until {
        gate.next_ready = until;
    }
}

impl Default for EasyedaApi {
    fn default() -> Self {
        Self::new()
    }
}

fn should_retry_app_error(error: &AppError) -> bool {
    match error {
        AppError::Easyeda(EasyedaError::ApiRequest(error)) => {
            error.is_timeout()
                || error.is_connect()
                || error.status().is_some_and(is_retryable_status)
        }
        AppError::Easyeda(EasyedaError::HttpStatus {
            status, message, ..
        }) => is_upstream_block_status(*status, message) || is_retryable_status(*status),
        AppError::Easyeda(EasyedaError::ApiResponse { message, .. }) => {
            is_retryable_api_message(message)
        }
        _ => false,
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

fn is_upstream_block_status(status: StatusCode, message: &str) -> bool {
    status == StatusCode::FORBIDDEN && message.contains("HTML error page")
}

fn is_retryable_api_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "busy",
        "rate",
        "timeout",
        "temporar",
        "try again",
        "too many",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn retry_delay_for_attempt(attempt: u32) -> Duration {
    Duration::from_millis(RETRY_BASE_DELAY_MS.saturating_mul(1u64 << attempt.min(8)))
}

fn retry_delay_for_error(error: &AppError, attempt: u32) -> Duration {
    match error {
        AppError::Easyeda(EasyedaError::HttpStatus {
            status, message, ..
        }) if is_upstream_block_status(*status, message) => {
            Duration::from_millis(UPSTREAM_BLOCK_RETRY_DELAY_MS.saturating_mul(attempt as u64 + 1))
        }
        _ => retry_delay_for_attempt(attempt),
    }
}

async fn read_response_body(
    response: reqwest::Response,
    stall_timeout_secs: u64,
) -> std::result::Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    let stall_timeout = Duration::from_secs(stall_timeout_secs);

    loop {
        match tokio::time::timeout(stall_timeout, stream.next()).await {
            Ok(Some(Ok(chunk))) => body.extend_from_slice(&chunk),
            Ok(Some(Err(error))) => return Err(error.to_string()),
            Ok(None) => return Ok(body),
            Err(_) => {
                return Err(format!(
                    "STEP download stalled for more than {}s",
                    stall_timeout_secs
                ));
            }
        }
    }
}

fn api_response_message(response: &ApiResponse) -> String {
    if let Some(message) = response.message.as_deref().map(str::trim)
        && !message.is_empty()
    {
        return message.to_string();
    }

    if let Some(code) = &response.code {
        return format!("code={code}");
    }

    "success=false without message".to_string()
}

fn summarize_http_error_body(message: String) -> String {
    let message = message.trim();
    if message.is_empty() {
        "empty response body".to_string()
    } else if looks_like_html(message) {
        "upstream returned an HTML error page".to_string()
    } else {
        message.chars().take(120).collect()
    }
}

fn looks_like_html(message: &str) -> bool {
    let lower = message
        .chars()
        .take(80)
        .collect::<String>()
        .to_ascii_lowercase();
    lower.contains("<!doctype html") || lower.contains("<html") || lower.contains("<head")
}

fn is_not_found_api_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not found")
        || message.contains("not exist")
        || message.contains("no data")
        || message.contains("component does not")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_html_forbidden_as_upstream_block() {
        let message =
            summarize_http_error_body("<html><head><title>Forbidden</title></head>".to_string());

        assert_eq!(message, "upstream returned an HTML error page");
        assert!(is_upstream_block_status(StatusCode::FORBIDDEN, &message));
        assert!(!is_upstream_block_status(StatusCode::NOT_FOUND, &message));
    }

    #[test]
    fn upstream_block_retry_delay_is_slow_path() {
        let error = AppError::Easyeda(EasyedaError::HttpStatus {
            lcsc_id: "C1".to_string(),
            status: StatusCode::FORBIDDEN,
            message: "upstream returned an HTML error page".to_string(),
        });

        assert_eq!(
            retry_delay_for_error(&error, 1),
            Duration::from_millis(UPSTREAM_BLOCK_RETRY_DELAY_MS * 2)
        );
    }
}
