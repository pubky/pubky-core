//! Auth service facade — orchestrates the full grant-based auth flow.
//!
//! Route handlers call `AuthService` methods instead of orchestrating
//! verification, persistence, and minting steps directly.

use axum::http::StatusCode;
use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::grant_session::{GrantSessionInfo, GrantSessionResponse},
    auth::jws::{GrantId, TokenId},
    capabilities::Action,
    crypto::Keypair,
};

use crate::{
    client_server::err_if_user_is_invalid::get_user_or_http_error,
    persistence::sql::{user::UserEntity, SqlDb},
    shared::{HttpError, HttpResult},
};

use super::auth::BearerSession;
use super::crypto::{
    access_jwt_issuer::AccessJwt,
    grant_verifier::Grant,
    jws_crypto::JwsCompact,
    pop_verifier::{PopProof, PopVerificationContext, POP_NONCE_GC_THRESHOLD_SECS},
};
use super::persistence::{
    grant::{GrantEntity, GrantRepository, NewGrant},
    grant_session::{GrantSessionRepository, NewGrantSession},
    pop_nonce::PopNonceRepository,
};
use super::routes::CreateGrantSessionRequest;
use crate::client_server::auth::AuthSession;

/// Default JWT lifetime: 1 hour.
const DEFAULT_JWT_LIFETIME_SECS: u64 = 3600;

/// Facade for all grant-based auth operations.
///
/// Constructed once and stored in `AppState`. Encapsulates the verify → persist
/// → mint pipeline so route handlers stay thin.
#[derive(Clone, Debug)]
pub struct AuthService {
    sql_db: SqlDb,
    homeserver_keypair: Keypair,
}

impl AuthService {
    pub fn new(sql_db: SqlDb, homeserver_keypair: Keypair) -> Self {
        Self {
            sql_db,
            homeserver_keypair,
        }
    }

    /// The homeserver's public key (used for JWT audience checks and session info).
    pub fn homeserver_public_key(&self) -> pubky_common::crypto::PublicKey {
        self.homeserver_keypair.public_key()
    }

    /// Full grant-based session creation: verify grant → find user → verify PoP
    /// → check nonce replay → check revocation → store grant → mint JWT.
    pub async fn create_grant_session(
        &self,
        request: CreateGrantSessionRequest,
    ) -> HttpResult<GrantSessionResponse> {
        let grant = self.verify_grant(&request.grant)?;
        let user = self.find_user(&grant).await?;
        let pop = self.verify_pop_proof(&request.pop, &grant)?;
        self.check_nonce_replay(&pop).await?;
        self.check_grant_not_revoked(&grant).await?;
        self.store_grant(&grant, &user).await?;
        self.mint_session(&grant).await
    }

    /// Revoke a grant and delete all its sessions.
    pub async fn revoke_grant(&self, grant_id: &GrantId) -> HttpResult<()> {
        GrantRepository::revoke(grant_id, &mut self.sql_db.pool().into()).await?;
        GrantSessionRepository::delete_all_for_grant(grant_id, &mut self.sql_db.pool().into())
            .await?;
        Ok(())
    }

    /// List all active (non-revoked, non-expired) grants for a user.
    pub async fn list_active_grants(
        &self,
        user_id: i32,
    ) -> HttpResult<Vec<GrantEntity>> {
        let grants =
            GrantRepository::list_active_for_user(user_id, &mut self.sql_db.pool().into()).await?;
        Ok(grants)
    }

    /// Sign out a bearer session: revoke its grant and delete all sessions.
    pub async fn signout_bearer(&self, bearer: &BearerSession) -> HttpResult<()> {
        self.revoke_grant(&bearer.grant_id).await
    }

    /// Resolve the database user ID from an auth session.
    pub async fn resolve_user_id(&self, auth: &AuthSession) -> HttpResult<i32> {
        match auth {
            AuthSession::Bearer(b) => {
                let grant = GrantRepository::get_by_grant_id(
                    &b.grant_id,
                    &mut self.sql_db.pool().into(),
                )
                .await?;
                Ok(grant.user_id)
            }
            AuthSession::Cookie(c) => Ok(c.session.user_id),
        }
    }

    /// Check that the session has root capability.
    pub fn require_root_capability(auth: &AuthSession) -> HttpResult<()> {
        let has_root = auth.capabilities().iter().any(|cap| {
            cap.scope == "/"
                && cap.actions.contains(&Action::Read)
                && cap.actions.contains(&Action::Write)
        });

        if has_root {
            Ok(())
        } else {
            Err(HttpError::forbidden_with_message(
                "Root capability required",
            ))
        }
    }

    // ── Private helpers ─────────────────────────────────────────────────

    fn verify_grant(&self, compact: &JwsCompact) -> HttpResult<Grant> {
        Grant::verify(compact).map_err(|e| {
            HttpError::new_with_message(StatusCode::BAD_REQUEST, format!("Invalid grant: {e}"))
        })
    }

    async fn find_user(&self, grant: &Grant) -> HttpResult<UserEntity> {
        get_user_or_http_error(&grant.issuer_key, &mut self.sql_db.pool().into(), false).await
    }

    fn verify_pop_proof(&self, compact: &JwsCompact, grant: &Grant) -> HttpResult<PopProof> {
        let hs_pubkey_z32 = self.homeserver_keypair.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &grant.cnf_key,
            expected_audience: &hs_pubkey_z32,
            expected_grant_id: &grant.grant_id,
        };
        PopProof::verify(compact, &context).map_err(|e| {
            HttpError::new_with_message(StatusCode::UNAUTHORIZED, format!("Invalid PoP proof: {e}"))
        })
    }

    async fn check_nonce_replay(&self, pop: &PopProof) -> HttpResult<()> {
        let _ = PopNonceRepository::garbage_collect(
            POP_NONCE_GC_THRESHOLD_SECS,
            &mut self.sql_db.pool().into(),
        )
        .await;

        PopNonceRepository::check_and_track(&pop.nonce, &mut self.sql_db.pool().into())
            .await
            .map_err(|_| {
                HttpError::new_with_message(StatusCode::UNAUTHORIZED, "PoP nonce already used")
            })
    }

    async fn check_grant_not_revoked(&self, grant: &Grant) -> HttpResult<()> {
        match GrantRepository::is_revoked(&grant.grant_id, &mut self.sql_db.pool().into()).await {
            Ok(true) => Err(HttpError::new_with_message(
                StatusCode::UNAUTHORIZED,
                "Grant has been revoked",
            )),
            Ok(false) => Ok(()),
            Err(sqlx::Error::RowNotFound) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    async fn store_grant(&self, grant: &Grant, user: &UserEntity) -> HttpResult<()> {
        let new_grant = NewGrant {
            grant_id: grant.grant_id.clone(),
            user_id: user.id,
            client_id: grant.client_id.clone(),
            client_cnf_key: grant.cnf_key.z32(),
            capabilities: grant.capabilities.clone(),
            issued_at: grant.issued_at.timestamp() as u64,
            expires_at: grant.expires_at.timestamp() as u64,
        };
        GrantRepository::create(&new_grant, &mut self.sql_db.pool().into()).await?;
        Ok(())
    }

    async fn mint_session(
        &self,
        grant: &Grant,
    ) -> HttpResult<GrantSessionResponse> {
        let now = Utc::now().timestamp() as u64;
        let token_id = TokenId::generate();
        let jwt_exp = now + DEFAULT_JWT_LIFETIME_SECS;

        let raw_jwt = AccessJwtClaims {
            iss: self.homeserver_keypair.public_key(),
            sub: grant.issuer_key.clone(),
            gid: grant.grant_id.clone(),
            jti: token_id.clone(),
            iat: now,
            exp: jwt_exp,
        };

        let token = AccessJwt::mint(&self.homeserver_keypair, &raw_jwt);

        let new_session = NewGrantSession {
            token_id: token_id.clone(),
            grant_id: grant.grant_id.clone(),
            expires_at: jwt_exp,
        };
        GrantSessionRepository::create(&new_session, &mut self.sql_db.pool().into()).await?;

        Ok(GrantSessionResponse {
            token,
            session: GrantSessionInfo {
                homeserver: self.homeserver_keypair.public_key(),
                pubky: grant.issuer_key.clone(),
                client_id: grant.client_id.clone(),
                capabilities: grant.capabilities.to_vec(),
                grant_id: grant.grant_id.clone(),
                token_expires_at: jwt_exp,
                grant_expires_at: grant.expires_at.timestamp() as u64,
                created_at: now,
            },
        })
    }
}
