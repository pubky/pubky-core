//! Auth service facade — orchestrates the full grant-based auth flow.
//!
//! Route handlers call `AuthService` methods instead of orchestrating
//! verification, persistence, and minting steps directly.

use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::grant_session::{GrantSessionInfo, GrantSessionResponse},
    auth::jws::GrantId,
    auth::jws::TokenId,
    crypto::{Keypair, PublicKey},
};
use crate::{
    persistence::sql::{
        signup_code::{SignupCodeId, SignupCodeRepository},
        uexecutor, UnifiedExecutor,
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

    /// Full grant-based session creation: verify → find user → store → mint.
    pub async fn create_grant_session(
        &self,
        grant_jws: &JwsCompact,
        pop_jws: &JwsCompact,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let grant = self.verify_grant_and_pop(grant_jws, pop_jws).await?;
        let user = self.find_user(&grant).await?;
        self.store_and_mint(&grant, &user).await
    }

    /// Grant-based signup: verify → create user → store → mint (all-or-nothing).
    pub async fn signup_grant_session(
        &self,
        grant_jws: &JwsCompact,
        pop_jws: &JwsCompact,
        signup_mode: &SignupMode,
        signup_token: Option<&str>,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let grant = self.verify_grant_and_pop(grant_jws, pop_jws).await?;
        let mut tx = self.sql_db.pool().begin().await?;
        let user = Self::create_user_in_tx(&grant.issuer_key, signup_mode, signup_token, &mut tx).await?;
        Self::store_grant(&grant, &user, uexecutor!(tx)).await?;
        let response = self.mint_session(&grant, uexecutor!(tx)).await?;
        tx.commit().await?;
        Ok(response)
    }

    /// Revoke a grant and delete all its sessions atomically.
    pub async fn revoke_grant(&self, grant_id: &GrantId) -> Result<(), AuthServiceError> {
        let mut tx = self.sql_db.pool().begin().await?;
        GrantRepository::revoke(grant_id, uexecutor!(tx)).await?;
        GrantSessionRepository::delete_all_for_grant(grant_id, uexecutor!(tx)).await?;
        tx.commit().await?;
        Ok(())
    }

    /// List all active (non-revoked, non-expired) grants for a user.
    pub async fn list_active_grants(
        &self,
        user_id: i32,
    ) -> Result<Vec<GrantEntity>, AuthServiceError> {
        Ok(GrantRepository::list_active_for_user(user_id, &mut self.sql_db.pool().into()).await?)
    }

    /// Return session info for a bearer session.
    pub async fn get_bearer_session_info(
        &self,
        bearer: &BearerSession,
    ) -> Result<GrantSessionInfo, AuthServiceError> {
        let grant = self.get_grant(&bearer.grant_id).await?;

        Ok(GrantSessionInfo {
            homeserver: self.homeserver_keypair.public_key(),
            pubky: bearer.user_key.clone(),
            client_id: grant.client_id.clone(),
            capabilities: bearer.capabilities.to_vec(),
            grant_id: bearer.grant_id.clone(),
            token_expires_at: bearer.token_expires_at,
            grant_expires_at: grant.expires_at as u64,
            created_at: grant.created_at.and_utc().timestamp() as u64,
        })
    }

    /// Sign out: Revoke its grant and delete all sessions.
    pub async fn signout_bearer(&self, bearer: &BearerSession) -> Result<(), AuthServiceError> {
        self.revoke_grant(&bearer.grant_id).await
    }

    /// Resolve a verified Access JWT into a BearerSession.
    ///
    /// Looks up the session by token ID, validates the grant is active
    /// (not revoked, not expired), and returns the resolved session.
    pub async fn resolve_bearer_session(
        &self,
        jwt: &AccessJwt,
    ) -> Result<BearerSession, AuthServiceError> {
        let session =
            GrantSessionRepository::get_by_token_id(&jwt.token_id, &mut self.sql_db.pool().into())
                .await
                .map_err(|e| match e {
                    sqlx::Error::RowNotFound => AuthServiceError::SessionNotFound,
                    other => AuthServiceError::Internal(other),
                })?;

        let grant = self.get_grant(&jwt.grant_id).await?;

        if grant.revoked_at.is_some() {
            return Err(AuthServiceError::GrantRevoked);
        }

        let now = chrono::Utc::now().timestamp();
        if grant.expires_at <= now {
            return Err(AuthServiceError::GrantExpired);
        }

        Ok(BearerSession {
            user_key: jwt.user_key.clone(),
            capabilities: grant.capabilities,
            grant_id: jwt.grant_id.clone(),
            token_id: jwt.token_id.clone(),
            token_expires_at: session.expires_at as u64,
        })
    }

    /// Resolve the database user ID from an auth session.
    pub async fn resolve_user_id(&self, auth: &AuthSession) -> Result<i32, AuthServiceError> {
        match auth {
            AuthSession::Bearer(b) => {
                let grant = self.get_grant(&b.grant_id).await?;
                Ok(grant.user_id)
            }
            AuthSession::Cookie(c) => Ok(c.user_id),
        }
    }

    /// Check that the session has root capability.
    pub fn require_root_capability(auth: &AuthSession) -> Result<(), AuthServiceError> {
        if auth.capabilities().iter().any(|cap| cap.is_root()) {
            Ok(())
        } else {
            Err(AuthServiceError::RootCapabilityRequired)
        }
    }

    // ── Private helpers ─────────────────────────────────────────────────

    /// Shared verification pipeline: verify grant → check revocation → verify PoP → check nonce.
    async fn verify_grant_and_pop(
        &self,
        grant_jws: &JwsCompact,
        pop_jws: &JwsCompact,
    ) -> Result<Grant, AuthServiceError> {
        let grant = self.verify_grant(grant_jws)?;
        self.check_grant_not_revoked(&grant).await?;
        let pop = self.verify_pop_proof(pop_jws, &grant)?;
        self.check_nonce_replay(&pop).await?;
        Ok(grant)
    }

    /// Shared tail: persist grant → mint JWT session.
    /// No tx needed because store_grant is idempotent and mint_session only creates a session row.
    async fn store_and_mint(
        &self,
        grant: &Grant,
        user: &UserEntity,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        Self::store_grant(grant, user, &mut self.sql_db.pool().into()).await?;
        self.mint_session(grant, &mut self.sql_db.pool().into()).await
    }

    /// Look up a grant by ID. Returns `GrantNotFound` if missing.
    async fn get_grant(&self, grant_id: &GrantId) -> Result<GrantEntity, AuthServiceError> {
        match GrantRepository::get_by_grant_id(grant_id, &mut self.sql_db.pool().into()).await {
            Ok(grant) => Ok(grant),
            Err(sqlx::Error::RowNotFound) => Err(AuthServiceError::GrantNotFound),
            Err(e) => Err(AuthServiceError::Internal(e)),
        }
    }

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
    pub(crate) async fn create_new_user(
        &self,
        public_key: &PublicKey,
        signup_mode: &SignupMode,
        signup_token: Option<&str>,
    ) -> Result<UserEntity, AuthServiceError> {
        let mut tx = self.sql_db.pool().begin().await?;
        let user = Self::create_user_in_tx(public_key, signup_mode, signup_token, &mut tx).await?;
        tx.commit().await?;
        Ok(user)
    }

    /// Inner user-creation logic that participates in an existing transaction.
    async fn create_user_in_tx(
        public_key: &PublicKey,
        signup_mode: &SignupMode,
        signup_token: Option<&str>,
        tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    ) -> Result<UserEntity, AuthServiceError> {
        Self::ensure_user_not_exists(public_key, tx).await?;
        if *signup_mode == SignupMode::TokenRequired {
            Self::validate_and_consume_signup_token(signup_token, public_key, tx).await?;
        }
        let user = UserRepository::create(public_key, uexecutor!(*tx)).await?;
        Ok(user)
    }

    /// Reject if the user already exists. Passes if user is not found.
    async fn ensure_user_not_exists(
        public_key: &PublicKey,
        tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    ) -> Result<(), AuthServiceError> {
        match UserRepository::get(public_key, uexecutor!(*tx)).await {
            Ok(_) => Err(AuthServiceError::UserAlreadyExists),
            Err(sqlx::Error::RowNotFound) => Ok(()),
            Err(e) => Err(AuthServiceError::Internal(e)),
        }
    }

    /// Validate and consume a signup token within the given transaction.
    async fn validate_and_consume_signup_token(
        signup_token: Option<&str>,
        public_key: &PublicKey,
        tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    ) -> Result<(), AuthServiceError> {
        let token_str = signup_token.ok_or(AuthServiceError::SignupTokenRequired)?;
        let code_id = SignupCodeId::new(token_str.to_string())
            .map_err(|e| AuthServiceError::InvalidSignupTokenFormat(e.to_string()))?;
        let code = match SignupCodeRepository::get(&code_id, uexecutor!(*tx)).await {
            Ok(code) => code,
            Err(sqlx::Error::RowNotFound) => return Err(AuthServiceError::InvalidSignupToken),
            Err(e) => return Err(AuthServiceError::Internal(e)),
        };
        if code.used_by.is_some() {
            return Err(AuthServiceError::SignupTokenAlreadyUsed);
        }
        SignupCodeRepository::mark_as_used(&code_id, public_key, uexecutor!(*tx)).await?;
        Ok(())
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
        if let Err(e) = PopNonceRepository::garbage_collect(
            POP_NONCE_GC_THRESHOLD_SECS,
            &mut self.sql_db.pool().into(),
        )
        .await
        {
            tracing::warn!("PoP nonce garbage collection failed: {e}");
        }

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
    async fn store_grant<'a>(
        grant: &Grant,
        user: &UserEntity,
        executor: &mut UnifiedExecutor<'a>,
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
        GrantRepository::create(&new_grant, executor).await?;
        Ok(())
    }

    /// Mint a new access JWT and persist the session row.
    async fn mint_session<'a>(
        &self,
        grant: &Grant,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let now = Utc::now().timestamp() as u64;
        let token_id = TokenId::generate();
        let jwt_exp = now + DEFAULT_JWT_LIFETIME_SECS;

        let claims = build_access_jwt_claims(&self.homeserver_keypair, grant, &token_id, now, jwt_exp);
        let token = AccessJwt::mint(&self.homeserver_keypair, &claims);

        let new_session = NewGrantSession { token_id, grant_id: grant.grant_id.clone(), expires_at: jwt_exp };
        GrantSessionRepository::create(&new_session, executor).await?;

        Ok(build_session_response(token.to_string(), grant, self.homeserver_keypair.public_key(), jwt_exp, now))
    }
}

fn build_access_jwt_claims(
    keypair: &Keypair,
    grant: &Grant,
    token_id: &TokenId,
    now: u64,
    jwt_exp: u64,
) -> AccessJwtClaims {
    AccessJwtClaims {
        iss: keypair.public_key(),
        sub: grant.issuer_key.clone(),
        gid: grant.grant_id.clone(),
        jti: token_id.clone(),
        iat: now,
        exp: jwt_exp,
    }
}

fn build_session_response(
    token: String,
    grant: &Grant,
    homeserver: PublicKey,
    jwt_exp: u64,
    now: u64,
) -> GrantSessionResponse {
    GrantSessionResponse {
        token,
        session: GrantSessionInfo {
            homeserver,
            pubky: grant.issuer_key.clone(),
            client_id: grant.client_id.clone(),
            capabilities: grant.capabilities.to_vec(),
            grant_id: grant.grant_id.clone(),
            token_expires_at: jwt_exp,
            grant_expires_at: grant.expires_at.timestamp() as u64,
            created_at: now,
        },
    }
}
