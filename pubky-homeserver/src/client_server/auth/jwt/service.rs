//! Auth service facade — orchestrates the full grant-based auth flow.
//!
//! Route handlers call `AuthService` methods instead of orchestrating
//! verification, persistence, and minting steps directly.

use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::grant_session::{GrantSessionInfo, GrantSessionResponse},
    auth::jws::{GrantId, TokenId},
    capabilities::Action,
    crypto::{Keypair, PublicKey},
};

use crate::{
    persistence::sql::{
        signup_code::{SignupCodeId, SignupCodeRepository},
        uexecutor,
        user::{UserEntity, UserRepository},
        SqlDb,
    },
    SignupMode,
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
use super::service_error::AuthServiceError;
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
    /// Create a new auth service with the given database and homeserver keypair.
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
        grant_jws: &JwsCompact,
        pop_jws: &JwsCompact,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let grant = self.verify_grant(grant_jws)?;
        let user = self.find_user(&grant).await?;
        let pop = self.verify_pop_proof(pop_jws, &grant)?;
        self.check_nonce_replay(&pop).await?;
        self.check_grant_not_revoked(&grant).await?;
        self.store_grant(&grant, &user).await?;
        self.mint_session(&grant).await
    }

    /// Grant-based signup: verify grant → create user → verify PoP
    /// → check nonce replay → check revocation → store grant → mint JWT.
    ///
    /// Like [`create_grant_session`] but creates the user instead of requiring one.
    pub async fn signup_grant_session(
        &self,
        grant_jws: &JwsCompact,
        pop_jws: &JwsCompact,
        signup_mode: &SignupMode,
        signup_token: Option<&str>,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let grant = self.verify_grant(grant_jws)?;
        let pop = self.verify_pop_proof(pop_jws, &grant)?;
        let user = self
            .create_new_user(&grant.issuer_key, signup_mode, signup_token)
            .await?;
        self.check_nonce_replay(&pop).await?;
        self.check_grant_not_revoked(&grant).await?;
        self.store_grant(&grant, &user).await?;
        self.mint_session(&grant).await
    }

    /// Revoke a grant and delete all its sessions.
    pub async fn revoke_grant(&self, grant_id: &GrantId) -> Result<(), AuthServiceError> {
        GrantRepository::revoke(grant_id, &mut self.sql_db.pool().into()).await?;
        GrantSessionRepository::delete_all_for_grant(grant_id, &mut self.sql_db.pool().into())
            .await?;
        Ok(())
    }

    /// List all active (non-revoked, non-expired) grants for a user.
    pub async fn list_active_grants(
        &self,
        user_id: i32,
    ) -> Result<Vec<GrantEntity>, AuthServiceError> {
        let grants =
            GrantRepository::list_active_for_user(user_id, &mut self.sql_db.pool().into()).await?;
        Ok(grants)
    }

    /// Sign out a bearer session: revoke its grant and delete all sessions.
    pub async fn signout_bearer(&self, bearer: &BearerSession) -> Result<(), AuthServiceError> {
        self.revoke_grant(&bearer.grant_id).await
    }

    /// Resolve the database user ID from an auth session.
    pub async fn resolve_user_id(&self, auth: &AuthSession) -> Result<i32, AuthServiceError> {
        match auth {
            AuthSession::Bearer(b) => {
                let grant = GrantRepository::get_by_grant_id(
                    &b.grant_id,
                    &mut self.sql_db.pool().into(),
                )
                .await?;
                Ok(grant.user_id)
            }
            AuthSession::Cookie(c) => Ok(c.user_id),
        }
    }

    /// Check that the session has root capability.
    pub fn require_root_capability(auth: &AuthSession) -> Result<(), AuthServiceError> {
        let has_root = auth.capabilities().iter().any(|cap| {
            cap.scope == "/"
                && cap.actions.contains(&Action::Read)
                && cap.actions.contains(&Action::Write)
        });

        if has_root {
            Ok(())
        } else {
            Err(AuthServiceError::RootCapabilityRequired)
        }
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Verify the grant JWS signature, type header, and expiry.
    fn verify_grant(&self, compact: &JwsCompact) -> Result<Grant, AuthServiceError> {
        Ok(Grant::verify(compact)?)
    }

    /// Look up the user identified by the grant's `iss` claim. Returns error if not found.
    async fn find_user(&self, grant: &Grant) -> Result<UserEntity, AuthServiceError> {
        match UserRepository::get(&grant.issuer_key, &mut self.sql_db.pool().into()).await {
            Ok(user) => Ok(user),
            Err(sqlx::Error::RowNotFound) => Err(AuthServiceError::UserNotFound),
            Err(e) => Err(AuthServiceError::Internal(e)),
        }
    }

    /// Create a new user with optional signup token validation, all in one transaction.
    async fn create_new_user(
        &self,
        public_key: &PublicKey,
        signup_mode: &SignupMode,
        signup_token: Option<&str>,
    ) -> Result<UserEntity, AuthServiceError> {
        let mut tx = self.sql_db.pool().begin().await?;

        // User must NOT already exist
        match UserRepository::get(public_key, uexecutor!(tx)).await {
            Ok(_) => return Err(AuthServiceError::UserAlreadyExists),
            Err(sqlx::Error::RowNotFound) => {}
            Err(e) => return Err(AuthServiceError::Internal(e)),
        }

        // Validate signup token if required
        if *signup_mode == SignupMode::TokenRequired {
            let token_str = signup_token
                .ok_or(AuthServiceError::SignupTokenRequired)?;
            let signup_code_id = SignupCodeId::new(token_str.to_string()).map_err(|e| {
                AuthServiceError::InvalidSignupTokenFormat(e.to_string())
            })?;

            let code = match SignupCodeRepository::get(&signup_code_id, uexecutor!(tx)).await {
                Ok(code) => code,
                Err(sqlx::Error::RowNotFound) => {
                    return Err(AuthServiceError::InvalidSignupToken);
                }
                Err(e) => return Err(AuthServiceError::Internal(e)),
            };

            if code.used_by.is_some() {
                return Err(AuthServiceError::SignupTokenAlreadyUsed);
            }

            SignupCodeRepository::mark_as_used(&signup_code_id, public_key, uexecutor!(tx))
                .await?;
        }

        let user = UserRepository::create(public_key, uexecutor!(tx)).await?;
        tx.commit().await?;
        Ok(user)
    }

    /// Verify the PoP proof signature, audience, grant binding, and timestamp window.
    fn verify_pop_proof(
        &self,
        compact: &JwsCompact,
        grant: &Grant,
    ) -> Result<PopProof, AuthServiceError> {
        let hs_pubkey_z32 = self.homeserver_keypair.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &grant.cnf_key,
            expected_audience: &hs_pubkey_z32,
            expected_grant_id: &grant.grant_id,
        };
        Ok(PopProof::verify(compact, &context)?)
    }

    /// Reject replayed PoP nonces. Garbage-collects expired nonces first.
    async fn check_nonce_replay(&self, pop: &PopProof) -> Result<(), AuthServiceError> {
        let _ = PopNonceRepository::garbage_collect(
            POP_NONCE_GC_THRESHOLD_SECS,
            &mut self.sql_db.pool().into(),
        )
        .await;

        PopNonceRepository::check_and_track(&pop.nonce, &mut self.sql_db.pool().into())
            .await
            .map_err(|_| AuthServiceError::NonceReplay)
    }

    /// Verify the grant has not been revoked. A not-yet-stored grant passes (first use).
    async fn check_grant_not_revoked(&self, grant: &Grant) -> Result<(), AuthServiceError> {
        match GrantRepository::is_revoked(&grant.grant_id, &mut self.sql_db.pool().into()).await {
            Ok(true) => Err(AuthServiceError::GrantRevoked),
            Ok(false) => Ok(()),
            Err(sqlx::Error::RowNotFound) => Ok(()),
            Err(e) => Err(AuthServiceError::Internal(e)),
        }
    }

    /// Persist the grant idempotently (ON CONFLICT DO NOTHING).
    async fn store_grant(
        &self,
        grant: &Grant,
        user: &UserEntity,
    ) -> Result<(), AuthServiceError> {
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

    /// Mint a new access JWT and persist the session row.
    async fn mint_session(
        &self,
        grant: &Grant,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
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
