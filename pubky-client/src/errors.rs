pub use super::*;

#[derive(Debug, thiserror::Error)]
pub enum PubkyError {
    #[error("Error: {0}")]
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PkarrError {
    #[error("DNS error: {0}")]
    Dns(#[from] pkarr::dns::SimpleDnsError),

    #[error("DNS error: {0}")]
    SignPacket(#[from] pkarr::errors::SignedPacketBuildError),

    #[error("Publish failed: {0}")]
    Publish(#[from] pkarr::errors::PublishError),

    #[error("Query failed: {0}")]
    Query(#[from] pkarr::errors::QueryError),

    #[error("Build failed: {0}")]
    Build(#[from] pkarr::errors::BuildError),

    // Add more as needed
    #[error("Other pkarr error: {0}")]
    Other(String),
}

impl PkarrError {
    pub fn is_retryable(&self) -> bool {
        match self {
            PkarrError::Publish(_) => true,
            PkarrError::Query(_) => true,
            PkarrError::Build(_) => false,
            PkarrError::Other(_) => false,
            _ => false,
        }
    }
}

/// Convenience type alias
pub type PubkyResult<T> = Result<T, PubkyError>;
