//! JWT Bearer token authentication logic.
//!
//! Extracts and validates grant-based JWT Bearer tokens.

use axum::{body::Body, http::header, http::Request};
use pubky_common::auth::jws::{GrantId, TokenId};
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

use super::crypto::access_jwt_issuer::AccessJwt;
use super::crypto::jws_crypto::JwsCompact;
use crate::client_server::auth::AuthSession;
use crate::client_server::auth::AuthState;
use crate::shared::HttpError;

/// Grant-based JWT Bearer token session data.
#[derive(Clone, Debug)]
pub struct BearerSession {
    /// User public key.
    pub user_key: PublicKey,
    /// Capabilities from the underlying grant.
    pub capabilities: Capabilities,
    /// Grant ID (for revocation).
    pub grant_id: GrantId,
    /// Token ID (session cache key).
    pub token_id: TokenId,
    /// When the JWT token expires (Unix seconds).
    pub token_expires_at: u64,
}

/// Extract and parse Bearer token from the Authorization header.
///
/// - `Ok(Some(token))` — valid Bearer token found.
/// - `Ok(None)` — no Authorization header present.
/// - `Err(HttpError)` — Authorization header present but not a valid Bearer token.
pub fn extract_bearer_token(req: &Request<Body>) -> Result<Option<JwsCompact>, HttpError> {
    let Some(value) = req.headers().get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| HttpError::unauthorized_with_message("Malformed Authorization header"))?;

    let Some(raw_token) = value.strip_prefix("Bearer ") else {
        return Err(HttpError::unauthorized_with_message("Malformed Authorization header"));
    };
    let token = JwsCompact::parse(raw_token)
        .map_err(|_| HttpError::unauthorized_with_message("Malformed Bearer token"))?;
    Ok(Some(token))
}

/// Authenticate via grant-based JWT Bearer token.
///
/// Verifies the JWT signature and expiry, then delegates session and grant
/// lookup to `AuthService` (use-case layer).
pub async fn authenticate_bearer(
    state: &AuthState,
    token: &JwsCompact,
) -> Result<AuthSession, HttpError> {
    let jwt = AccessJwt::verify(token, &state.auth_service.homeserver_public_key())
        .map_err(|_| HttpError::unauthorized_with_message("Invalid or expired JWT"))?;

    let bearer = state
        .auth_service
        .resolve_bearer_session(&jwt)
        .await
        .map_err(HttpError::from)?;

    Ok(AuthSession::Bearer(bearer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use pubky_common::auth::access_jwt::AccessJwtClaims;
    use pubky_common::crypto::Keypair;

    fn mint_jwt(homeserver_keypair: &Keypair) -> String {
        let user_kp = Keypair::random();
        let now = chrono::Utc::now().timestamp() as u64;
        let claims = AccessJwtClaims {
            iss: homeserver_keypair.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: now,
            exp: now + 3600,
        };
        AccessJwt::mint(homeserver_keypair, &claims)
    }

    #[test]
    fn extract_bearer_no_auth_header() {
        let req = Request::builder().body(Body::empty()).unwrap();
        assert!(matches!(extract_bearer_token(&req), Ok(None)));
    }

    #[test]
    fn extract_bearer_basic_auth_rejected() {
        let req = Request::builder()
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_malformed_token() {
        let req = Request::builder()
            .header("Authorization", "Bearer not-a-jws")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_empty_token() {
        let req = Request::builder()
            .header("Authorization", "Bearer ")
            .body(Body::empty())
            .unwrap();
        assert!(extract_bearer_token(&req).is_err());
    }

    #[test]
    fn extract_bearer_valid_jws_format() {
        let req = Request::builder()
            .header("Authorization", "Bearer aaa.bbb.ccc")
            .body(Body::empty())
            .unwrap();
        let result = extract_bearer_token(&req).unwrap();
        assert!(result.is_some());
    }
}
