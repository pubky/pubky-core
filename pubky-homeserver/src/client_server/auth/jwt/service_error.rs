//! Domain errors for [`AuthService`](super::service::AuthService) operations.
//!
//! These errors capture business-logic failure modes without any HTTP coupling.
//! The `From<AuthServiceError> for HttpError` impl below maps them to status codes
//! at the HTTP boundary.

use axum::http::StatusCode;

use super::crypto::{grant_verifier, pop_verifier};
use crate::shared::HttpError;

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

    /// Session lacks root capability.
    #[error("Root capability required")]
    RootCapabilityRequired,

    /// Database or infrastructure error.
    #[error("Internal error: {0}")]
    Internal(#[from] sqlx::Error),
}

impl From<AuthServiceError> for HttpError {
    fn from(error: AuthServiceError) -> Self {
        match error {
            AuthServiceError::InvalidGrant(ref inner) => match inner {
                grant_verifier::Error::InvalidSignature | grant_verifier::Error::Expired => {
                    HttpError::unauthorized_with_message(error.to_string())
                }
                _ => HttpError::bad_request(error.to_string()),
            },
            AuthServiceError::InvalidPopProof(_) => {
                HttpError::unauthorized_with_message(error.to_string())
            }
            AuthServiceError::UserNotFound => HttpError::not_found(),
            AuthServiceError::UserAlreadyExists => {
                HttpError::new_with_message(StatusCode::CONFLICT, "User already exists")
            }
            AuthServiceError::SignupTokenRequired => HttpError::bad_request("Token required"),
            AuthServiceError::InvalidSignupTokenFormat(ref msg) => {
                HttpError::bad_request(format!("Invalid signup token format: {msg}"))
            }
            AuthServiceError::InvalidSignupToken => {
                HttpError::unauthorized_with_message("Invalid token")
            }
            AuthServiceError::SignupTokenAlreadyUsed => {
                HttpError::unauthorized_with_message("Token already used")
            }
            AuthServiceError::NonceReplay => {
                HttpError::unauthorized_with_message("PoP nonce already used")
            }
            AuthServiceError::GrantRevoked => {
                HttpError::unauthorized_with_message("Grant has been revoked")
            }
            AuthServiceError::RootCapabilityRequired => {
                HttpError::forbidden_with_message("Root capability required")
            }
            AuthServiceError::Internal(e) => {
                HttpError::internal_server_and_log(format!("Auth service: {e}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn test_auth_service_error_mapping() {
        // Grant verification: security errors -> UNAUTHORIZED
        let resp = HttpError::from(AuthServiceError::InvalidGrant(
            grant_verifier::Error::InvalidSignature,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::InvalidGrant(
            grant_verifier::Error::Expired,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Grant verification: malformed input -> BAD_REQUEST
        let resp = HttpError::from(AuthServiceError::InvalidGrant(
            grant_verifier::Error::InvalidFormat,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = HttpError::from(AuthServiceError::InvalidGrant(
            grant_verifier::Error::InvalidHeaderType,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = HttpError::from(AuthServiceError::InvalidGrant(
            grant_verifier::Error::InvalidTimestamp,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // PoP verification -> UNAUTHORIZED
        let resp = HttpError::from(AuthServiceError::InvalidPopProof(
            pop_verifier::Error::InvalidSignature,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::InvalidPopProof(
            pop_verifier::Error::AudienceMismatch,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::InvalidPopProof(
            pop_verifier::Error::InvalidFormat,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::InvalidPopProof(
            pop_verifier::Error::InvalidHeaderType,
        ))
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // User lookup
        let resp = HttpError::from(AuthServiceError::UserNotFound).into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Signup errors
        let resp = HttpError::from(AuthServiceError::UserAlreadyExists).into_response();
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        let resp = HttpError::from(AuthServiceError::SignupTokenRequired).into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp =
            HttpError::from(AuthServiceError::InvalidSignupTokenFormat("bad".into()))
                .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = HttpError::from(AuthServiceError::InvalidSignupToken).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::SignupTokenAlreadyUsed).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // Security failures
        let resp = HttpError::from(AuthServiceError::NonceReplay).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::GrantRevoked).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::RootCapabilityRequired).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
