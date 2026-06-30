use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid response from LCEDA: {0}")]
    InvalidResponse(String),

    #[error("no results found for keyword: {0}")]
    NoResults(String),

    #[error("invalid index {index}. Valid range: 1..{max} for keyword '{keyword}'.")]
    InvalidIndex {
        keyword: String,
        index: usize,
        max: usize,
    },

    #[error("selected component has no 3D model UUID")]
    MissingModelUuid,

    #[error("selected component has no symbol/footprint uuid")]
    MissingSymbolOrFootprint,

    #[error(transparent)]
    Easyeda(#[from] EasyedaError),

    #[error(transparent)]
    Kicad(#[from] KicadError),

    #[error(transparent)]
    Conversion(#[from] ConversionError),

    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Error)]
pub enum EasyedaError {
    #[error("API request failed: {0}")]
    ApiRequest(#[from] reqwest::Error),

    #[error("invalid LCSC ID format: {0}")]
    InvalidLcscId(String),

    #[error("component not found: {0}")]
    ComponentNotFound(String),

    #[error("EasyEDA returned HTTP {status} for {lcsc_id}: {message}")]
    HttpStatus {
        lcsc_id: String,
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("EasyEDA API rejected {lcsc_id}: {message}")]
    ApiResponse { lcsc_id: String, message: String },

    #[error("failed to parse JSON response: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("invalid component data: {0}")]
    InvalidData(String),
}

#[derive(Debug, Error)]
pub enum KicadError {
    #[error("failed to export symbol: {0}")]
    SymbolExport(String),

    #[error("failed to export footprint: {0}")]
    FootprintExport(String),

    #[error("failed to export 3D model: {0}")]
    ModelExport(String),

    #[error("invalid KiCad version")]
    InvalidVersion,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("invalid coordinate: {0}")]
    InvalidCoordinate(String),

    #[error("invalid unit conversion: {0}")]
    InvalidUnit(String),

    #[error("SVG path parse error: {0}")]
    SvgParse(String),

    #[error("arc conversion failed: {0}")]
    ArcConversion(String),
}
