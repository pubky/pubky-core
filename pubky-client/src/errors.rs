use pkarr::errors::{PublishError, QueryError, SignedPacketBuildError};
use thiserror::Error;

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

    #[error("Pkarr: Failed to build or sign packet")]
    PkarrPacketBuild(#[from] SignedPacketBuildError),

    #[error("Pkarr: Failed to publish record to the DHT")]
    PkarrPublish(#[from] PublishError),

    #[error("Pkarr: Failed to query the DHT")]
    PkarrQuery(#[from] QueryError),

    #[error("Invalid URL structure: {0}")]
    InvalidUrlStructure(String),

    #[error("Invalid domain name for Pkarr record")]
    InvalidDomain(#[from] pkarr::dns::SimpleDnsError),

    #[error("Failed to handle Session data")]
    Session(#[from] pubky_common::session::Error),

    #[error("Token verification failed: {0}")]
    VerificationFailed(String),

    #[error("Cryptography error: {0}")]
    Crypto(String),

    #[error("The provided auth request is no longer valid.")]
    AuthRequestExpired,
}

/// A specialized `Result` type for `pubky` operations.
pub type Result<T> = std::result::Result<T, Error>;

/// An error that can occur when building a `Client`.
#[derive(Error, Debug)]
pub enum BuildError {
    #[error(transparent)]
    PkarrBuildError(#[from] pkarr::errors::BuildError),
}
