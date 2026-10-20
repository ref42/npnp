use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::{Client, StatusCode, Url};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::lceda::easyeda_fallback;
use crate::lceda::models::SearchItem;
use crate::util::nested_string;

const SEARCH_API: &str = "https://pro.lceda.cn/api/szlcsc/eda/product/list";
const COMPONENT_API: &str = "https://pro.lceda.cn/api/components";
const STEP_API: &str = "https://modules.lceda.cn/qAxj6KHrDKw4blvCG8QJPs7Y";
const OBJ_API: &str = "https://modules.lceda.cn/3dmodel";
const MAX_REQUEST_ATTEMPTS: usize = 3;
const RETRY_BASE_DELAY_MS: u64 = 250;

#[derive(Debug, Clone)]
pub struct LcedaClient {
    client: Client,
}

impl LcedaClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(format!("npnp/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(35))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    pub async fn search_components(&self, keyword: &str) -> Result<Vec<SearchItem>> {
        let mut url = Url::parse(SEARCH_API)
            .map_err(|err| AppError::InvalidResponse(format!("bad search url: {err}")))?;
        url.query_pairs_mut().append_pair("wd", keyword);

        let payload = self.get_json(url.as_str()).await?;
        let Some(results) = payload.get("result").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };

        let mut items = Vec::with_capacity(results.len());
        for (index, raw) in results.iter().enumerate() {
            let attrs = raw.get("attributes").unwrap_or(&Value::Null);
            items.push(SearchItem {
                index: index + 1,
                display_title: raw
                    .get("display_title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                title: raw
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                manufacturer: attrs
                    .get("Manufacturer")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                model_uuid: attrs
                    .get("3D Model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                raw: raw.clone(),
            });
        }

        Ok(items)
    }

    pub async fn select_item(&self, keyword: &str, index: usize) -> Result<SearchItem> {
        let items = self.search_components(keyword).await?;
        if items.is_empty() && index == 1 && is_lcsc_id_keyword(keyword) {
            return easyeda_fallback::select_item_by_lcsc_id(keyword.trim()).await;
        }
        select_from_items(keyword, index, &items)
    }

    pub async fn component_detail(&self, uuid: &str) -> Result<Value> {
        if easyeda_fallback::is_easyeda_uuid(uuid) {
            return easyeda_fallback::component_detail(uuid).await;
        }
        let mut url = Url::parse(&format!("{COMPONENT_API}/{uuid}"))
            .map_err(|err| AppError::InvalidResponse(format!("bad component url: {err}")))?;
        url.query_pairs_mut().append_pair("uuid", uuid);
        self.get_json(url.as_str()).await
    }

    pub async fn get_model_uuid(&self, item: &SearchItem) -> Result<String> {
        let Some(seed_uuid) = item.model_uuid.as_deref() else {
            return Err(AppError::MissingModelUuid);
        };
        if easyeda_fallback::is_easyeda_step_uuid(seed_uuid) {
            return Ok(seed_uuid.to_string());
        }

        let detail = self.component_detail(seed_uuid).await?;
        let code = detail
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        if code == 0 {
            if let Some(model_uuid) = nested_string(&detail, &["result", "3d_model_uuid"]) {
                return Ok(model_uuid);
            }
        }

        Ok(seed_uuid.to_string())
    }

    pub async fn download_step_bytes(&self, model_uuid: &str) -> Result<Arc<Vec<u8>>> {
        if let Some(uuid) = easyeda_fallback::easyeda_step_uuid(model_uuid) {
            return crate::easyeda::EasyedaApi::new()
                .download_3d_step(uuid)
                .await;
        }
        let url = format!("{STEP_API}/{model_uuid}");
        Ok(Arc::new(self.get_bytes(&url).await?))
    }

    /// Downloads a model straight into `path`. A batch asks for one model per
    /// parallel part, so writing them to disk as they arrive keeps the batch's peak
    /// from following the number of parts.
    pub async fn download_step_bytes_to_file(&self, model_uuid: &str, path: &Path) -> Result<()> {
        if let Some(uuid) = easyeda_fallback::easyeda_step_uuid(model_uuid) {
            return crate::easyeda::EasyedaApi::new()
                .download_3d_step_to_file(uuid, path)
                .await;
        }
        let url = format!("{STEP_API}/{model_uuid}");
        let bytes = self.get_bytes(&url).await?;
        fs::write(path, bytes)?;
        Ok(())
    }

    pub async fn download_obj_bytes(&self, model_uuid: &str) -> Result<Vec<u8>> {
        if easyeda_fallback::is_easyeda_step_uuid(model_uuid) {
            return Err(AppError::Other(
                "OBJ download is not available for EasyEDA fallback 3D models".to_string(),
            ));
        }
        let url = format!("{OBJ_API}/{model_uuid}");
        self.get_bytes(&url).await
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let bytes = self.get_bytes(url).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let mut last_error = None;

        for attempt in 0..MAX_REQUEST_ATTEMPTS {
            match self.get_bytes_once(url).await {
                Ok(bytes) => return Ok(bytes),
                Err(err)
                    if attempt + 1 < MAX_REQUEST_ATTEMPTS && should_retry_request_error(&err) =>
                {
                    last_error = Some(err);
                    tokio::time::sleep(retry_delay_for_attempt(attempt)).await;
                }
                Err(err) => return Err(err.into()),
            }
        }

        Err(last_error
            .expect("retry loop should preserve the last error")
            .into())
    }

    async fn get_bytes_once(&self, url: &str) -> std::result::Result<Vec<u8>, reqwest::Error> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

fn should_retry_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.status().is_some_and(is_retryable_status)
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 425 | 429 | 500 | 502 | 503 | 504)
}

fn retry_delay_for_attempt(attempt: usize) -> Duration {
    Duration::from_millis(RETRY_BASE_DELAY_MS.saturating_mul(1u64 << attempt.min(8)))
}

fn select_from_items(keyword: &str, index: usize, items: &[SearchItem]) -> Result<SearchItem> {
    if items.is_empty() {
        return Err(AppError::NoResults(keyword.to_string()));
    }
    if !(1..=items.len()).contains(&index) {
        return Err(AppError::InvalidIndex {
            keyword: keyword.to_string(),
            index,
            max: items.len(),
        });
    }

    if index == 1 {
        if let Some(item) = find_exact_lcsc_id_match(keyword, items) {
            return Ok(item.clone());
        }
    }

    Ok(items[index - 1].clone())
}

fn find_exact_lcsc_id_match<'a>(keyword: &str, items: &'a [SearchItem]) -> Option<&'a SearchItem> {
    let keyword = keyword.trim();
    if !is_lcsc_id_keyword(keyword) {
        return None;
    }

    items.iter().find(|item| {
        item.lcsc_id()
            .as_deref()
            .is_some_and(|lcsc_id| lcsc_id.eq_ignore_ascii_case(keyword))
    })
}

fn is_lcsc_id_keyword(keyword: &str) -> bool {
    let trimmed = keyword.trim();
    let Some(digits) = trimmed
        .strip_prefix('C')
        .or_else(|| trimmed.strip_prefix('c'))
    else {
        return false;
    };

    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

impl Default for LcedaClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_lcsc_id_keyword, is_retryable_status, retry_delay_for_attempt, select_from_items,
    };
    use crate::lceda::SearchItem;
    use reqwest::StatusCode;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn selects_exact_lcsc_id_match_for_default_index() {
        let items = vec![
            item(1, "Almost Match", "C20400"),
            item(2, "Exact Match", "C2040"),
        ];

        let selected = select_from_items("C2040", 1, &items).expect("select exact item");
        assert_eq!(selected.display_name(), "Exact Match");
        assert_eq!(selected.index, 2);
    }

    #[test]
    fn explicit_non_default_index_keeps_index_selection() {
        let items = vec![item(1, "Exact Match", "C2040"), item(2, "Other", "C9999")];

        let selected = select_from_items("C2040", 2, &items).expect("select indexed item");
        assert_eq!(selected.display_name(), "Other");
        assert_eq!(selected.index, 2);
    }

    #[test]
    fn recognizes_exact_lcsc_id_keywords() {
        assert!(is_lcsc_id_keyword("C2040"));
        assert!(is_lcsc_id_keyword(" c2040 "));
        assert!(!is_lcsc_id_keyword("C"));
        assert!(!is_lcsc_id_keyword("C20A40"));
        assert!(!is_lcsc_id_keyword("RP2040"));
    }

    #[test]
    fn retries_transient_http_status_codes() {
        for status in [
            StatusCode::REQUEST_TIMEOUT,
            StatusCode::TOO_EARLY,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert!(
                is_retryable_status(status),
                "expected retryable status: {status}"
            );
        }
    }

    #[test]
    fn does_not_retry_permanent_http_status_codes() {
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            assert!(
                !is_retryable_status(status),
                "expected non-retryable status: {status}"
            );
        }
    }

    #[test]
    fn retry_delay_grows_per_attempt() {
        assert_eq!(retry_delay_for_attempt(0), Duration::from_millis(250));
        assert_eq!(retry_delay_for_attempt(1), Duration::from_millis(500));
        assert_eq!(retry_delay_for_attempt(2), Duration::from_millis(1000));
    }

    fn item(index: usize, display_title: &str, lcsc_id: &str) -> SearchItem {
        SearchItem {
            index,
            display_title: display_title.to_string(),
            title: String::new(),
            manufacturer: String::new(),
            model_uuid: None,
            raw: json!({"product_code": lcsc_id}),
        }
    }
}
