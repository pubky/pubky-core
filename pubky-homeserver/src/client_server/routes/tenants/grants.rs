//! Grant management endpoints (root only).
//!
//! These endpoints allow Ring (or any root-capability session) to list
//! active grants and revoke specific grants.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::{
    capabilities::Action,
    auth::jws::GrantId,
};
use serde::Serialize;

use crate::{
    client_server::{
        middleware::authentication::AuthSession,
        AppState,
    },
    persistence::sql::{
        grant::{GrantEntity, GrantRepository},
        grant_session::GrantSessionRepository,
    },
    shared::{HttpError, HttpResult},
};

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
    State(state): State<AppState>,
    auth: AuthSession,
) -> HttpResult<impl IntoResponse> {
    require_root_capability(&auth)?;

    let user_id = resolve_user_id(&state, &auth).await?;
    let grants =
        GrantRepository::list_active_for_user(user_id, &mut state.sql_db.pool().into()).await?;

    let infos: Vec<GrantInfo> = grants.into_iter().map(GrantInfo::from).collect();
    Ok(Json(infos))
}

/// `DELETE /session/{gid}` — revoke a specific grant and all its sessions.
///
/// Requires root capability.
pub async fn revoke_grant(
    State(state): State<AppState>,
    auth: AuthSession,
    Path(grant_id): Path<String>,
) -> HttpResult<impl IntoResponse> {
    require_root_capability(&auth)?;

    let grant_id =
        GrantId::parse(&grant_id).map_err(|_| {
            HttpError::new_with_message(StatusCode::BAD_REQUEST, "Invalid grant ID format")
        })?;

    // Revoke grant
    GrantRepository::revoke(&grant_id, &mut state.sql_db.pool().into()).await?;

    // Delete all sessions minted from this grant
    GrantSessionRepository::delete_all_for_grant(&grant_id, &mut state.sql_db.pool().into())
        .await?;

    Ok(StatusCode::OK)
}

/// Check that the authenticated session has root capability (/:rw).
fn require_root_capability(auth: &AuthSession) -> HttpResult<()> {
    let has_root = auth.capabilities().iter().any(|cap| {
        cap.scope == "/" && cap.actions.contains(&Action::Read) && cap.actions.contains(&Action::Write)
    });

    if has_root {
        Ok(())
    } else {
        Err(HttpError::forbidden_with_message(
            "Root capability required",
        ))
    }
}

/// Resolve the database user ID from the auth session.
async fn resolve_user_id(state: &AppState, auth: &AuthSession) -> HttpResult<i32> {
    match auth {
        AuthSession::Bearer(b) => {
            let grant = GrantRepository::get_by_grant_id(
                &b.grant_id,
                &mut state.sql_db.pool().into(),
            )
            .await?;
            Ok(grant.user_id)
        }
        AuthSession::Cookie(c) => Ok(c.session.user_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use pubky_common::{
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
        auth::jws::{ClientId, GrantId, TokenId},
    };

    use crate::client_server::middleware::authentication::{AuthSession, BearerSession};

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
        assert!(require_root_capability(&auth).is_ok());
    }

    #[test]
    fn test_require_root_capability_rejects_read_only() {
        let auth = bearer_auth(
            Capabilities::builder()
                .read("/")
                .finish(),
        );
        let err = require_root_capability(&auth).unwrap_err();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn test_require_root_capability_rejects_scoped_rw() {
        let auth = bearer_auth(
            Capabilities::builder()
                .read_write("/pub/app/")
                .finish(),
        );
        let err = require_root_capability(&auth).unwrap_err();
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
