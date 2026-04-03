//! JWT Bearer token authentication logic.
//!
//! Extracts and validates grant-based JWT Bearer tokens.

use pubky_common::auth::jws::{GrantId, TokenId};
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

use super::crypto::access_jwt_issuer::verify_access_jwt;
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

/// Authenticate via grant-based JWT Bearer token.
///
/// Verifies the JWT signature and expiry, then delegates session and grant
/// lookup to `AuthService` (use-case layer).
pub async fn authenticate_bearer(
    state: &AuthState,
    token: &JwsCompact,
) -> Result<AuthSession, HttpError> {
    let jwt = verify_access_jwt(token, &state.auth_service.homeserver_public_key())
        .map_err(|_| HttpError::unauthorized_with_message("Invalid or expired JWT"))?;

    let bearer = state
        .auth_service
        .resolve_bearer_session(&jwt)
        .await
        .map_err(HttpError::from)?;

    Ok(AuthSession::Bearer(bearer))
}
