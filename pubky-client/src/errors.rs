use thiserror::Error;

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

    #[error("Pkarr record is malformed or missing required data: {0}")]
    InvalidRecord(String),
}

impl PkarrError {
    /// Returns true if the error is from a DHT operation that might succeed by simply retrying.
    pub fn is_retryable(&self) -> bool {
        matches!(self, PkarrError::Publish(_) | PkarrError::Query(_))
    }
}

// --- Consolidated URL Error ---
#[derive(Debug, Error)]
pub enum UrlError {
    #[error("Failed to parse URL: {0}")]
    Parse(#[from] url::ParseError),

    #[error("Invalid URL structure: {0}")]
    InvalidStructure(String),
}

// --- Consolidated Authentication Error ---
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Session handling failed: {0}")]
    Session(#[from] pubky_common::session::Error),

    #[error("Token verification failed: {0}")]
    VerificationFailed(String),

    #[error("Cryptography error: {0}")]
    Crypto(String),

    #[error("General authentication error: {0}")]
    Validation(String),

    #[error("The provided auth request has expired or was cancelled.")]
    RequestExpired,
}

// --- The Main Operational Error Enum ---
#[derive(Debug, Error)]
pub enum Error {
    #[error("HTTP request failed with status {status}: {message}")]
    HttpStatus {
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("HTTP transport error")]
    Http(#[from] reqwest::Error),

    #[error("Pkarr operation failed: {0}")]
    Pkarr(#[from] PkarrError),

    #[error("URL error: {0}")]
    Url(#[from] UrlError),

    #[error("Authentication error: {0}")]
    Authentication(#[from] AuthError),

    #[error("Homeserver not found for the given public key")]
    HomeserverNotFound,
}

/// A specialized `Result` type for `pubky` operations.
pub type Result<T> = std::result::Result<T, Error>;

// --- Ergonomic "Staircase" From Implementations ---
// A macro to reduce boilerplate for converting base errors into the top-level Error.
macro_rules! impl_from_for_error {
    ($from_type:ty, $to_variant:path) => {
        impl From<$from_type> for Error {
            fn from(err: $from_type) -> Self {
                $to_variant(err.into())
            }
        }
    };
}

// Pkarr Errors
impl_from_for_error!(pkarr::errors::SignedPacketBuildError, Error::Pkarr);
impl_from_for_error!(pkarr::errors::PublishError, Error::Pkarr);
impl_from_for_error!(pkarr::errors::QueryError, Error::Pkarr);
impl_from_for_error!(pkarr::dns::SimpleDnsError, Error::Pkarr);

// URL Errors
impl_from_for_error!(url::ParseError, Error::Url);

// Auth Errors
impl_from_for_error!(pubky_common::session::Error, Error::Authentication);
