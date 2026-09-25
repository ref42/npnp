pub mod altium;
pub mod app;
pub mod batch;
pub mod cli;
pub mod easyeda;
pub mod error;
pub mod kicad;
pub mod lceda;
pub mod lcsc;
pub mod metadata;
pub mod step_model;
pub mod util;

pub use altium::{footprint, merge, pcblib, schlib};
pub use cli::{Cli, Commands};
pub use error::{AppError, Result};
pub use lceda::{LcedaClient, SearchItem};
pub use lcsc::{LcscClient, LcscProduct};
