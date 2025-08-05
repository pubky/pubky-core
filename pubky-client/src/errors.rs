use thiserror::Error;

/// The primary error type for the `pubky` library.
#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed")]
    Http(#[from] reqwest::Error),

    #[error("Invalid URL")]
    Url(#[from] url::ParseError),

    #[error("Pkarr operation failed")]
    Pkarr(#[from] pkarr::errors::Error),

    #[error("Invalid domain name for Pkarr record")]
    InvalidDomain(#[from] pkarr::dns::domain::DomainError),

    #[error("Failed to (de)serialize data")]
    Serialization(#[from] serde_json::Error),

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
