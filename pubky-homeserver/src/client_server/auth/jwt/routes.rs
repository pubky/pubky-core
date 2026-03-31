//! JWT/grant-based route handlers.
//!
//! Grant session creation and grant management endpoints.

use axum::{
    extract::{Path, State},
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

// ── Grant session creation ─────────────────────────────────────────────────

/// JSON request body for grant-based session creation.
#[derive(Deserialize)]
pub struct CreateGrantSessionRequest {
    /// Grant JWS (user-signed).
    pub grant: JwsCompact,
    /// PoP proof JWS (client-signed).
    pub pop: JwsCompact,
}

/// Handle `POST /session` with JSON body (grant-based auth).
pub async fn create_grant_session(
    State(state): State<AuthState>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let response = state.auth_service.create_grant_session(request).await?;
    Ok(Json(response))
}

// ── Grant management ───────────────────────────────────────────────────────

/// Summary of an active grant, returned by `GET /sessions`.
#[derive(Serialize)]
pub struct GrantInfo {
    pub grant_id: GrantId,
    pub client_id: String,
    pub capabilities: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl From<GrantEntity> for GrantInfo {
    fn from(entity: GrantEntity) -> Self {
        Self {
            grant_id: entity.grant_id,
            client_id: entity.client_id.to_string(),
            capabilities: entity.capabilities.to_string(),
            issued_at: entity.issued_at,
            expires_at: entity.expires_at,
        }
    }
}

/// `GET /sessions` — list all active grants for the authenticated user.
///
/// Requires root capability.
pub async fn list_grants(
    State(state): State<AuthState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    AuthService::require_root_capability(&auth)?;

    let user_id = state.auth_service.resolve_user_id(&auth).await?;
    let grants = state.auth_service.list_active_grants(user_id).await?;

    let infos: Vec<GrantInfo> = grants.into_iter().map(GrantInfo::from).collect();
    Ok(Json(infos))
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

    state.auth_service.revoke_grant(&grant_id).await?;
    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use pubky_common::{
        auth::jws::{ClientId, GrantId, TokenId},
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
    };

    use crate::client_server::auth::jwt::auth::BearerSession;

    fn bearer_auth(caps: Capabilities) -> AuthSession {
        AuthSession::Bearer(BearerSession {
            user_key: Keypair::random().public_key(),
            capabilities: caps,
            grant_id: GrantId::generate(),
            token_id: TokenId::generate(),
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
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_require_root_capability_rejects_scoped_rw() {
        let auth = bearer_auth(Capabilities::builder().read_write("/pub/app/").finish());
        let err = AuthService::require_root_capability(&auth).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_grant_info_from_entity() {
        let grant_id = GrantId::generate();
        let client_id = ClientId::new("example.app").unwrap();
        let caps = Capabilities::builder().cap(Capability::root()).finish();

        let entity = GrantEntity {
            id: 1,
            grant_id: grant_id.clone(),
            user_id: 42,
            user_pubkey: Keypair::random().public_key(),
            client_id: client_id.clone(),
            client_cnf_key: "cnf".to_string(),
            capabilities: caps.clone(),
            issued_at: 1000,
            expires_at: 2000,
            revoked_at: None,
            created_at: chrono::Utc::now().naive_utc(),
        };

        let info = GrantInfo::from(entity);
        assert_eq!(info.grant_id, grant_id);
        assert_eq!(info.client_id, client_id.to_string());
        assert_eq!(info.capabilities, caps.to_string());
        assert_eq!(info.issued_at, 1000);
        assert_eq!(info.expires_at, 2000);
    }
}
