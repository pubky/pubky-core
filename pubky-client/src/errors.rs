use thiserror::Error;

// --- Main Operational Errors ---
/// The primary error type for the `pubky` library.
#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed with status {status}: {message}")]
    HttpStatus {
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("HTTP transport error")]
    Http(#[from] reqwest::Error),

    #[error("Invalid URL")]
    Url(#[from] url::ParseError),

    #[error("Pkarr operation failed: {0}")]
    Pkarr(#[from] PkarrError),

    #[error("Invalid URL structure: {0}")]
    InvalidUrlStructure(String),

    #[error("Failed to handle Session data")]
    Session(#[from] pubky_common::session::Error),

    #[error("Token verification failed: {0}")]
    VerificationFailed(String),

    #[error("Cryptography error: {0}")]
    Crypto(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("The provided auth request is no longer valid.")]
    AuthRequestExpired,
}

// --- Build-Time Error ---
// This error can only happen during the client construction phase.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("Failed to build the Pkarr client: {0}")]
    Pkarr(#[from] pkarr::errors::BuildError),
}

// --- Pkarr Operational Errors ---
// A dedicated enum for all errors that can occur when interacting with Pkarr at runtime.
#[derive(Debug, Error)]
pub enum PkarrError {
    #[error("DNS operation failed: {0}")]
    Dns(#[from] pkarr::dns::SimpleDnsError),

    #[error("Failed to build or sign DNS packet: {0}")]
    SignPacket(#[from] pkarr::errors::SignedPacketBuildError),

    #[error("Failed to publish record to the DHT: {0}")]
    Publish(#[from] pkarr::errors::PublishError),

    #[error("Failed to query the DHT: {0}")]
    Query(#[from] pkarr::errors::QueryError),
}

impl PkarrError {
    /// Returns true if the error is from a DHT operation that might succeed by simply retrying.
    pub fn is_retryable(&self) -> bool {
        matches!(self, PkarrError::Publish(_) | PkarrError::Query(_))
    }
}

/// A specialized `Result` type for `pubky` operations.
pub type Result<T> = std::result::Result<T, Error>;
