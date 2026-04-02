//! HTTP error mapping for [`AuthServiceError`].
//!
//! Converts domain-level auth errors into HTTP status codes at the adapter boundary.

use axum::http::StatusCode;

use super::crypto::grant_verifier;
use super::service_error::AuthServiceError;
use crate::shared::HttpError;

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
            AuthServiceError::UserNotFound | AuthServiceError::GrantNotFound => {
                HttpError::not_found()
            }
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
            AuthServiceError::GrantExpired => {
                HttpError::unauthorized_with_message("Grant has expired")
            }
            AuthServiceError::SessionNotFound => {
                HttpError::unauthorized_with_message("Session not found")
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
    use crate::client_server::auth::jwt::crypto::pop_verifier;
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

        // Grant lookup
        let resp = HttpError::from(AuthServiceError::GrantNotFound).into_response();
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

        let resp = HttpError::from(AuthServiceError::GrantExpired).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::SessionNotFound).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let resp = HttpError::from(AuthServiceError::RootCapabilityRequired).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
