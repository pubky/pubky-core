//! JWT/grant-based route handlers.
//!
//! Grant session creation and grant management endpoints.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::auth::grant_session::GrantSessionInfo;
use pubky_common::auth::jws::GrantId;
use serde::{Deserialize, Serialize};

use super::crypto::jws_crypto::JwsCompact;
use super::persistence::grant::{GrantEntity, GrantRepository};
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
    Query(params): Query<HashMap<String, String>>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let response = state
        .auth_service
        .signup_grant_session(
            &request.grant,
            &request.pop,
            &state.signup_mode,
            params.get("signup_token").map(|s| s.as_str()),
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
    let AuthSession::Bearer(bearer) = auth else {
        return Err(HttpError::unauthorized());
    };

    let grant = GrantRepository::get_by_grant_id(
        &bearer.grant_id,
        &mut state.sql_db.pool().into(),
    )
    .await
    .map_err(|_| HttpError::not_found())?;

    let info = GrantSessionInfo {
        homeserver: state.auth_service.homeserver_public_key(),
        pubky: bearer.user_key.clone(),
        client_id: grant.client_id.clone(),
        capabilities: bearer.capabilities.to_vec(),
        grant_id: bearer.grant_id.clone(),
        token_expires_at: 0, // TODO: store in bearer session
        grant_expires_at: grant.expires_at as u64,
        created_at: grant.created_at.and_utc().timestamp() as u64,
    };
    Ok(Json(info))
}

/// `DELETE /auth/jwt/session` — revokes the grant and all its sessions.
pub async fn signout(
    State(state): State<AuthState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    let AuthSession::Bearer(bearer) = auth else {
        return Err(HttpError::unauthorized());
    };
    state.auth_service.signout_bearer(&bearer).await?;
    Ok(StatusCode::OK)
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
