use core::convert::Infallible;
use thiserror::Error;

// --- Build-Time Error ---
// This error can only happen during the client construction phase.
#[derive(Debug, Error)]
pub enum BuildError {
    #[error("Failed to build the Pkarr client: {0}")]
    Pkarr(#[from] pkarr::errors::BuildError),
    #[error("Failed to build the HTTP client: {0}")]
    Http(#[from] reqwest::Error),
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

// --- Consolidated Authentication Error ---
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Session handling failed: {0}")]
    Session(#[from] pubky_common::session::Error),

    #[error("Token verification failed: {0}")]
    VerificationFailed(#[from] pubky_common::auth::Error),

    #[error("Cryptography error: {0}")]
    DecryptError(#[from] pubky_common::crypto::DecryptError),

    #[error("General authentication error: {0}")]
    Validation(String),

    #[error("The provided auth request has expired or was cancelled.")]
    RequestExpired,
}

// --- Consolidated Request Error ---
#[derive(Debug, Error)]
pub enum RequestError {
    #[error("HTTP transport error: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("Server responded with an error: {status} - {message}")]
    Server {
        status: reqwest::StatusCode,
        message: String,
    },

    #[error("Invalid request/URI: {message}")]
    Validation { message: String },
}

// --- The Main Operational Error Enum ---
#[derive(Debug, Error)]
pub enum Error {
    #[error("Request failed: {0}")]
    Request(#[from] RequestError),

    #[error("Pkarr operation failed: {0}")]
    Pkarr(#[from] PkarrError),

    #[error("Failed to parse URL: {0}")]
    Parse(#[from] url::ParseError),

    #[error("Authentication error: {0}")]
    Authentication(#[from] AuthError),
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

// Auth Errors
impl_from_for_error!(pubky_common::session::Error, Error::Authentication);
impl_from_for_error!(pubky_common::auth::Error, Error::Authentication);
impl_from_for_error!(pubky_common::crypto::DecryptError, Error::Authentication);

// Request Errors
impl_from_for_error!(reqwest::Error, Error::Request);
