pub use super::*;

#[derive(Debug, thiserror::Error)]
pub enum PubkyError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("PKarr operation failed: {0}")]
    Pkarr(#[from] PkarrError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("URL parsing error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Homeserver not found")]
    HomeserverNotFound,

    #[error("Invalid relay")]
    InvalidRelay,

    #[error("Authentication failure")]
    AuthFailure,

    #[error("Invalid Pubky token: {0}")]
    InvalidPubkyToken(#[from] pubky_common::auth::Error),

    #[error("Access denied")] // not specifying error for privacy and security reasons
    AccessDenied,
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
}

impl PkarrError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, PkarrError::Publish(_) | PkarrError::Query(_))
    }
}

/// Convenience type alias
pub type Result<T> = std::result::Result<T, PubkyError>;
