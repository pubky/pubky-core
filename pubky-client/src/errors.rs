pub use super::*;
use pkarr::errors::{BuildError, QueryError, PublishError};

#[derive(Debug, thiserror::Error)]
pub enum PubkyError {
    #[error("Error: {0}")]
    Error(String),
}

#[cfg(not(wasm_browser))]
#[derive(Debug, thiserror::Error)]
pub enum PkarrError {
    #[error("Publish operation failed: {0}")]
    Publish(#[from] pkarr::errors::PublishError),

    #[error("Query operation failed: {0}")]
    Query(#[from] pkarr::errors::QueryError),

    #[error("Build operation failed: {0}")]
    Build(#[from] pkarr::errors::BuildError),
}
