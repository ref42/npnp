#[derive(Debug, Clone, Default)]
pub struct LcscProduct {
    pub sku: String,
    pub mpn: Option<String>,
    pub manufacturer: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub datasheet_url: Option<String>,
    pub properties: Vec<LcscProperty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LcscProperty {
    pub name: String,
    pub value: String,
}
