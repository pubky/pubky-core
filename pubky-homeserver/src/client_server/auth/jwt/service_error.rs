//! Domain errors for [`AuthService`](super::service::AuthService) operations.
//!
//! These errors capture business-logic failure modes without any HTTP coupling.
//! The HTTP status code mapping lives in [`error_mapping`](super::error_mapping).

use super::crypto::{grant_verifier, pop_verifier};

/// Domain errors from auth service operations.
#[derive(Debug, thiserror::Error)]
pub enum AuthServiceError {
    /// Grant JWS verification failed (signature, format, or expiry).
    #[error("Invalid grant: {0}")]
    InvalidGrant(#[from] grant_verifier::Error),

    /// PoP proof verification failed.
    #[error("Invalid PoP proof: {0}")]
    InvalidPopProof(#[from] pop_verifier::Error),

    /// User not found for the given public key.
    #[error("User not found")]
    UserNotFound,

    /// User already exists (signup conflict).
    #[error("User already exists")]
    UserAlreadyExists,

    /// Grant not found for the given grant ID.
    #[error("Grant not found")]
    GrantNotFound,

    /// Signup token is required but was not provided.
    #[error("Token required")]
    SignupTokenRequired,

    /// Signup token format is invalid.
    #[error("Invalid signup token format: {0}")]
    InvalidSignupTokenFormat(String),

    /// Signup token not found or invalid.
    #[error("Invalid token")]
    InvalidSignupToken,

    /// Signup token has already been used.
    #[error("Token already used")]
    SignupTokenAlreadyUsed,

    /// PoP nonce was already used (replay attack).
    #[error("PoP nonce already used")]
    NonceReplay,

    /// Grant has been revoked.
    #[error("Grant has been revoked")]
    GrantRevoked,

    /// Grant has expired.
    #[error("Grant has expired")]
    GrantExpired,

    /// Session not found for the given token ID.
    #[error("Session not found")]
    SessionNotFound,

    /// Session lacks root capability.
    #[error("Root capability required")]
    RootCapabilityRequired,

    /// Database or infrastructure error.
    #[error("Internal error: {0}")]
    Internal(#[from] sqlx::Error),
}
