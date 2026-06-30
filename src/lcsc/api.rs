use std::time::Duration;

use reqwest::Client;
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::lcsc::{LcscProduct, LcscProperty};

const PRODUCT_DETAIL_BASE: &str = "https://www.lcsc.com/product-detail";
const OPTIONAL_METADATA_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct LcscClient {
    client: Client,
}

impl LcscClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(format!(
                "npnp/{} (+https://github.com/linkyourbin/npnp)",
                env!("CARGO_PKG_VERSION")
            ))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(35))
            .build()
            .expect("failed to build reqwest client");
        Self { client }
    }

    pub async fn product_detail(&self, lcsc_id: &str) -> Result<LcscProduct> {
        let id = lcsc_id.trim();
        if id.is_empty() {
            return Err(AppError::Other(
                "missing LCSC ID for English metadata".to_string(),
            ));
        }
        let url = format!("{PRODUCT_DETAIL_BASE}/{id}.html");
        let html = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        parse_product_json_ld(&html).ok_or_else(|| {
            AppError::InvalidResponse(format!("LCSC English product metadata not found for {id}"))
        })
    }

    pub async fn product_detail_optional(&self, lcsc_id: &str) -> Option<LcscProduct> {
        match tokio::time::timeout(OPTIONAL_METADATA_TIMEOUT, self.product_detail(lcsc_id)).await {
            Ok(Ok(product)) => Some(product),
            Ok(Err(_)) | Err(_) => None,
        }
    }
}

impl Default for LcscClient {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn parse_product_json_ld(html: &str) -> Option<LcscProduct> {
    for script in json_ld_scripts(html) {
        let Ok(value) = serde_json::from_str::<Value>(&script) else {
            continue;
        };
        let Some(product) = product_from_json_ld(&value) else {
            continue;
        };
        return Some(product);
    }
    None
}

fn json_ld_scripts(html: &str) -> Vec<String> {
    let mut scripts = Vec::new();
    let mut rest = html;
    while let Some(script_index) = rest.find("<script") {
        rest = &rest[script_index..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let tag = &rest[..=tag_end];
        let is_json_ld =
            tag.contains("application/ld+json") || tag.contains("application\\/ld+json");
        rest = &rest[tag_end + 1..];
        let Some(close_index) = rest.find("</script>") else {
            break;
        };
        if is_json_ld {
            scripts.push(rest[..close_index].trim().to_string());
        }
        rest = &rest[close_index + "</script>".len()..];
    }
    scripts
}

fn product_from_json_ld(value: &Value) -> Option<LcscProduct> {
    let object = value.as_object()?;
    if !json_ld_type_matches(value, "Product") {
        return None;
    }

    let sku = object
        .get("sku")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if sku.is_empty() {
        return None;
    }

    let manufacturer = object.get("brand").and_then(|brand| {
        brand
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| brand.as_str())
    });

    let mut properties = Vec::new();
    if let Some(items) = object.get("additionalProperty").and_then(Value::as_array) {
        for item in items {
            let Some(name) = item.get("name").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Some(value) = item.get("value").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if !name.is_empty() && !value.is_empty() && value != "-" {
                properties.push(LcscProperty {
                    name: name.to_string(),
                    value: value.to_string(),
                });
            }
        }
    }

    Some(LcscProduct {
        sku,
        mpn: non_empty_string(object.get("mpn").and_then(Value::as_str)),
        manufacturer: non_empty_string(manufacturer),
        description: non_empty_string(object.get("description").and_then(Value::as_str)),
        category: non_empty_string(object.get("category").and_then(Value::as_str)),
        datasheet_url: object
            .get("subjectOf")
            .and_then(|subject| subject.get("url"))
            .and_then(Value::as_str)
            .and_then(|url| non_empty_string(Some(url))),
        properties,
    })
}

fn json_ld_type_matches(value: &Value, expected: &str) -> bool {
    match value.get("@type") {
        Some(Value::String(kind)) => kind == expected,
        Some(Value::Array(kinds)) => kinds
            .iter()
            .any(|kind| kind.as_str().is_some_and(|kind| kind == expected)),
        _ => false,
    }
}

fn non_empty_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{LcscProperty, parse_product_json_ld};

    #[test]
    fn parses_product_json_ld_from_lcsc_page() {
        let html = r#"
<html><head>
<script type="application/ld+json">{"@context":"http://schema.org","@type":"Product","name":"DORABO DB2ERC-2.54-4P-GN","sku":"C2927505","mpn":"DB2ERC-2.54-4P-GN","brand":{"@type":"Brand","name":"DORABO"},"description":"4 Position Board Side/Socket - Closed 2.54mm Pitch Right-Angle Pin","category":"Connectors/Headers, Plugs and Sockets","additionalProperty":[{"@type":"PropertyValue","name":"Package","value":"Through Hole,Right Angle,P=2.54mm"},{"@type":"PropertyValue","name":"Color","value":"Green"},{"@type":"PropertyValue","name":"Termination Style","value":"-"}],"subjectOf":{"@type":"DigitalDocument","name":"Datasheet","url":"https://datasheet.lcsc.com/datasheet/pdf/file.pdf?productCode=C2927505"}}</script>
</head></html>"#;

        let product = parse_product_json_ld(html).expect("product metadata");
        assert_eq!(product.sku, "C2927505");
        assert_eq!(product.mpn.as_deref(), Some("DB2ERC-2.54-4P-GN"));
        assert_eq!(product.manufacturer.as_deref(), Some("DORABO"));
        assert_eq!(
            product.description.as_deref(),
            Some("4 Position Board Side/Socket - Closed 2.54mm Pitch Right-Angle Pin")
        );
        assert_eq!(
            product.properties,
            vec![
                LcscProperty {
                    name: "Package".to_string(),
                    value: "Through Hole,Right Angle,P=2.54mm".to_string()
                },
                LcscProperty {
                    name: "Color".to_string(),
                    value: "Green".to_string()
                }
            ]
        );
    }

    #[tokio::test]
    async fn optional_metadata_ignores_empty_id() {
        let product = super::LcscClient::new().product_detail_optional("").await;
        assert!(product.is_none());
    }
}
