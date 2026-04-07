//! JWT/grant-based route handlers.
//!
//! Grant session creation and grant management endpoints.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::auth::jws::GrantId;
use serde::{Deserialize, Serialize};

use super::crypto::jws_crypto::JwsCompact;
use super::persistence::grant::GrantEntity;
use super::service::AuthService;
use crate::client_server::auth::AuthSession;
use crate::client_server::auth::AuthState;
use crate::shared::{HttpError, HttpResult};

// ── Request/response types ─────────────────────────────────────────────────

/// JSON request body for grant-based session creation.
#[derive(Deserialize)]
pub struct CreateGrantSessionRequest {
    /// Grant JWS (user-signed).
    pub grant: JwsCompact,
    /// PoP proof JWS (client-signed).
    pub pop: JwsCompact,
}

/// Query parameters for the signup endpoint.
#[derive(Deserialize)]
pub(crate) struct SignupParams {
    signup_token: Option<String>,
}

/// Summary of an active grant, returned by `GET /sessions`.
#[derive(Serialize)]
struct GrantInfo {
    grant_id: GrantId,
    client_id: String,
    capabilities: String,
    issued_at: u64,
    expires_at: u64,
}

impl From<GrantEntity> for GrantInfo {
    fn from(g: GrantEntity) -> Self {
        Self {
            grant_id: g.grant_id,
            client_id: g.client_id.to_string(),
            capabilities: g.capabilities.to_string(),
            issued_at: g.issued_at as u64,
            expires_at: g.expires_at as u64,
        }
    }
}

// ── Grant session creation ─────────────────────────────────────────────────

/// `POST /auth/jwt/session` — exchange grant + PoP for a JWT.
pub async fn create_grant_session(
    State(state): State<AuthState>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let response = state
        .auth_service
        .create_grant_session(&request.grant, &request.pop)
        .await?;
    Ok(Json(response))
}

/// `POST /auth/jwt/signup` — create a new user and return a JWT session.
///
/// Same input as session creation (grant + PoP), but creates the user first.
/// Optional `signup_token` query param when signup tokens are required.
pub async fn signup(
    State(state): State<AuthState>,
    Query(params): Query<SignupParams>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let response = state
        .auth_service
        .signup_grant_session(
            &request.grant,
            &request.pop,
            &state.signup_mode,
            params.signup_token.as_deref(),
        )
        .await?;
    Ok(Json(response))
}

// ── Session info & signout ─────────────────────────────────────────────────

/// `GET /auth/jwt/session` — returns grant session info as JSON.
pub async fn get_session(
    State(state): State<AuthState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let AuthSession::Grant(session) = auth else {
        return Err(HttpError::unauthorized());
    };
    let info = state.auth_service.get_grant_session_info(&session).await?;
    Ok(Json(info))
}

/// `DELETE /auth/jwt/session` — revokes the grant and all its sessions.
pub async fn signout(
    State(state): State<AuthState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let AuthSession::Grant(session) = auth else {
        return Err(HttpError::unauthorized());
    };
    state.auth_service.signout_grant_session(&session).await?;
    Ok(StatusCode::OK)
}

// ── Grant management ───────────────────────────────────────────────────────

/// `GET /sessions` — list all active grants for the authenticated user.
///
/// Requires root capability.
pub async fn list_grants(
    State(state): State<AuthState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    AuthService::require_root_capability(&auth)?;

    let user_id = state.auth_service.resolve_user_id(&auth).await?;
    let grants: Vec<GrantInfo> = state
        .auth_service
        .list_active_grants(user_id)
        .await?
        .into_iter()
        .map(GrantInfo::from)
        .collect();
    Ok(Json(grants))
}

/// `DELETE /session/{gid}` — revoke a specific grant and all its sessions.
///
/// Requires root capability.
pub async fn revoke_grant(
    State(state): State<AuthState>,
    auth: AuthSession,
    Path(grant_id): Path<String>,
) -> HttpResult<impl IntoResponse> {
    AuthService::require_root_capability(&auth)?;

    let grant_id = GrantId::parse(&grant_id).map_err(|_| {
        HttpError::new_with_message(StatusCode::BAD_REQUEST, "Invalid grant ID format")
    })?;

    state.auth_service.revoke_user_grant(&grant_id, &auth).await?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use pubky_common::{
        auth::jws::{GrantId, TokenId},
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
    };

    use crate::client_server::auth::jwt::auth::GrantSession;

    fn bearer_auth(caps: Capabilities) -> AuthSession {
        let now = chrono::Utc::now().timestamp() as u64;
        AuthSession::Grant(GrantSession {
            user_key: Keypair::random().public_key(),
            capabilities: caps,
            grant_id: GrantId::generate(),
            token_id: TokenId::generate(),
            token_expires_at: now + 3600,
        })
    }

    #[test]
    fn test_require_root_capability_accepts_root() {
        let auth = bearer_auth(Capabilities::builder().cap(Capability::root()).finish());
        assert!(AuthService::require_root_capability(&auth).is_ok());
    }

    #[test]
    fn test_require_root_capability_rejects_read_only() {
        let auth = bearer_auth(Capabilities::builder().read("/").finish());
        let err = AuthService::require_root_capability(&auth).unwrap_err();
        let resp = HttpError::from(err).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_require_root_capability_rejects_scoped_rw() {
        let auth = bearer_auth(Capabilities::builder().read_write("/pub/app/").finish());
        let err = AuthService::require_root_capability(&auth).unwrap_err();
        let resp = HttpError::from(err).into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
