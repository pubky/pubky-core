//! Auth service facade — orchestrates the full grant-based auth flow.
//!
//! Route handlers call `AuthService` methods instead of orchestrating
//! verification, persistence, and minting steps directly.

use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::grant::GrantClaims,
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

use super::auth::GrantSession;
use super::crypto::{
    access_jwt_issuer::{mint_access_jwt, verify_access_jwt},
    grant_verifier::verify_grant,
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
        let user = Self::create_user_in_tx(&grant.iss, signup_mode, signup_token, &mut tx).await?;
        Self::store_grant(&grant, &user, uexecutor!(tx)).await?;
        let response = self.mint_session(&grant, uexecutor!(tx)).await?;
        tx.commit().await?;
        Ok(response)
    }

    /// Revoke a grant after verifying it belongs to the authenticated user.
    pub async fn revoke_user_grant(
        &self,
        grant_id: &GrantId,
        auth: &AuthSession,
    ) -> Result<(), AuthServiceError> {
        let user_id = self.resolve_user_id(auth).await?;
        let grant = self.get_grant(grant_id).await?;
        if grant.user_id != user_id {
            return Err(AuthServiceError::GrantOwnershipMismatch);
        }
        self.revoke_grant(grant_id).await
    }

    /// Revoke a grant and delete all its sessions atomically.
    async fn revoke_grant(&self, grant_id: &GrantId) -> Result<(), AuthServiceError> {
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

    /// Return session info for a grant session.
    pub async fn get_grant_session_info(
        &self,
        session: &GrantSession,
    ) -> Result<GrantSessionInfo, AuthServiceError> {
        let grant = self.get_grant(&session.grant_id).await?;

        Ok(GrantSessionInfo {
            homeserver: self.homeserver_keypair.public_key(),
            pubky: session.user_key.clone(),
            client_id: grant.client_id.clone(),
            capabilities: session.capabilities.to_vec(),
            grant_id: session.grant_id.clone(),
            token_expires_at: session.token_expires_at,
            grant_expires_at: grant.expires_at as u64,
            created_at: grant.created_at.and_utc().timestamp() as u64,
        })
    }

    /// Sign out: Revoke its grant and delete all sessions.
    pub async fn signout_grant_session(&self, session: &GrantSession) -> Result<(), AuthServiceError> {
        self.revoke_grant(&session.grant_id).await
    }

    /// Resolve a verified Access JWT into a GrantSession.
    ///
    /// Looks up the session by token ID, validates the grant is active
    /// (not revoked, not expired), and returns the resolved session.
    pub async fn resolve_grant_session(
        &self,
        jwt: &AccessJwtClaims,
    ) -> Result<GrantSession, AuthServiceError> {
        let session = map_not_found(
            GrantSessionRepository::get_by_token_id(&jwt.jti, &mut self.sql_db.pool().into())
                .await,
            AuthServiceError::SessionNotFound,
        )?;

        let grant = self.get_grant(&jwt.gid).await?;
        grant.require_active(Utc::now().timestamp())?;

        Ok(GrantSession {
            user_key: jwt.sub.clone(),
            capabilities: grant.capabilities,
            grant_id: jwt.gid.clone(),
            token_id: jwt.jti.clone(),
            token_expires_at: session.expires_at as u64,
        })
    }

    /// Resolve the database user ID from an auth session.
    pub async fn resolve_user_id(&self, auth: &AuthSession) -> Result<i32, AuthServiceError> {
        match auth {
            AuthSession::Grant(b) => {
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
    ) -> Result<GrantClaims, AuthServiceError> {
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
        grant: &GrantClaims,
        user: &UserEntity,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        Self::store_grant(grant, user, &mut self.sql_db.pool().into()).await?;
        self.mint_session(grant, &mut self.sql_db.pool().into()).await
    }

    /// Look up a grant by ID. Returns `GrantNotFound` if missing.
    async fn get_grant(&self, grant_id: &GrantId) -> Result<GrantEntity, AuthServiceError> {
        map_not_found(
            GrantRepository::get_by_grant_id(grant_id, &mut self.sql_db.pool().into()).await,
            AuthServiceError::GrantNotFound,
        )
    }

    /// Verify the grant JWS signature, type header, and expiry.
    fn verify_grant(&self, compact: &JwsCompact) -> Result<GrantClaims, AuthServiceError> {
        Ok(verify_grant(compact)?)
    }

    /// Look up the user identified by the grant's `iss` claim. Returns error if not found.
    async fn find_user(&self, grant: &GrantClaims) -> Result<UserEntity, AuthServiceError> {
        map_not_found(
            UserRepository::get(&grant.iss, &mut self.sql_db.pool().into()).await,
            AuthServiceError::UserNotFound,
        )
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
            Err(e) => Err(e.into()),
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
        grant: &GrantClaims,
    ) -> Result<PopProof, AuthServiceError> {
        let hs_pubkey_z32 = self.homeserver_keypair.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &grant.cnf,
            expected_audience: &hs_pubkey_z32,
            expected_grant_id: &grant.jti,
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
    async fn check_grant_not_revoked(&self, grant: &GrantClaims) -> Result<(), AuthServiceError> {
        let is_revoked = GrantRepository::is_revoked(&grant.jti, &mut self.sql_db.pool().into())
            .await
            .or_else(|e| match e {
                sqlx::Error::RowNotFound => Ok(false),
                other => Err(AuthServiceError::Internal(other)),
            })?;
        if is_revoked {
            return Err(AuthServiceError::GrantRevoked);
        }
        Ok(())
    }

    /// Persist the grant idempotently (ON CONFLICT DO NOTHING).
    async fn store_grant<'a>(
        grant: &GrantClaims,
        user: &UserEntity,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), AuthServiceError> {
        let new_grant = NewGrant {
            grant_id: grant.jti.clone(),
            user_id: user.id,
            client_id: grant.client_id.clone(),
            client_cnf_key: grant.cnf.z32(),
            capabilities: grant.caps.clone().into(),
            issued_at: grant.iat,
            expires_at: grant.exp,
        };
        GrantRepository::create(&new_grant, executor).await?;
        Ok(())
    }

    /// Mint a new access JWT and persist the session row.
    async fn mint_session<'a>(
        &self,
        grant: &GrantClaims,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<GrantSessionResponse, AuthServiceError> {
        let now = Utc::now().timestamp() as u64;
        let token_id = TokenId::generate();
        let jwt_exp = now + DEFAULT_JWT_LIFETIME_SECS;

        let claims = build_access_jwt_claims(&self.homeserver_keypair, grant, &token_id, now, jwt_exp);
        let token = mint_access_jwt(&self.homeserver_keypair, &claims);

        let new_session = NewGrantSession { token_id, grant_id: grant.jti.clone(), expires_at: jwt_exp };
        GrantSessionRepository::create(&new_session, executor).await?;

        Ok(build_session_response(token.to_string(), grant, self.homeserver_keypair.public_key(), jwt_exp, now))
    }
}

fn build_access_jwt_claims(
    keypair: &Keypair,
    grant: &GrantClaims,
    token_id: &TokenId,
    now: u64,
    jwt_exp: u64,
) -> AccessJwtClaims {
    AccessJwtClaims {
        iss: keypair.public_key(),
        sub: grant.iss.clone(),
        gid: grant.jti.clone(),
        jti: token_id.clone(),
        iat: now,
        exp: jwt_exp,
    }
}

/// Map a `sqlx::Error::RowNotFound` to a specific domain error.
fn map_not_found<T>(
    result: Result<T, sqlx::Error>,
    not_found_err: AuthServiceError,
) -> Result<T, AuthServiceError> {
    result.map_err(|e| match e {
        sqlx::Error::RowNotFound => not_found_err,
        other => AuthServiceError::Internal(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::{
        auth::jws::{ClientId, GrantId, PopNonce},
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
    };
    use super::super::crypto::{
        jws_crypto,
        pop_verifier::PopProofClaims,
    };
    use crate::persistence::sql::SqlDb;

    // ── Helpers ─────────────────────────────────────────────────────

    async fn test_service() -> AuthService {
        let db = SqlDb::test().await;
        let hs_kp = Keypair::random();
        AuthService::new(db, hs_kp)
    }

    async fn create_test_user(service: &AuthService) -> (Keypair, i32) {
        let kp = Keypair::random();
        let user = UserRepository::create(&kp.public_key(), &mut service.sql_db.pool().into())
            .await
            .unwrap();
        (kp, user.id)
    }

    fn sign_grant(user_kp: &Keypair, client_kp: &Keypair, hs_pubkey: &PublicKey) -> (JwsCompact, JwsCompact, GrantClaims) {
        let now = Utc::now().timestamp() as u64;
        let raw_grant = GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: GrantId::generate(),
            iat: now,
            exp: now + 3600,
        };
        let grant_jws = sign_jws(user_kp, "pubky-grant", &raw_grant);

        let pop_claims = PopProofClaims {
            aud: hs_pubkey.clone(),
            gid: raw_grant.jti.clone(),
            nonce: PopNonce::generate(),
            iat: now,
        };
        let pop_jws = sign_jws(client_kp, "pubky-pop", &pop_claims);

        (grant_jws, pop_jws, raw_grant)
    }

    fn sign_jws<T: serde::Serialize>(kp: &Keypair, typ: &str, claims: &T) -> JwsCompact {
        let header = jws_crypto::eddsa_header(typ);
        let enc = jws_crypto::encoding_key(kp);
        let token = jsonwebtoken::encode(&header, claims, &enc).unwrap();
        JwsCompact::parse(&token).unwrap()
    }

    // ── create_grant_session ────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn create_grant_session_happy_path() {
        let service = test_service().await;
        let (user_kp, _user_id) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();
        assert!(!response.token.is_empty());
        assert_eq!(response.session.pubky, user_kp.public_key());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn create_grant_session_user_not_found() {
        let service = test_service().await;
        let user_kp = Keypair::random(); // user not created
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let err = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::UserNotFound));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn create_grant_session_invalid_grant_signature() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let wrong_signer = Keypair::random();

        // Sign grant with wrong key
        let (_, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());
        let now = Utc::now().timestamp() as u64;
        let raw_grant = GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: GrantId::generate(),
            iat: now,
            exp: now + 3600,
        };
        let bad_grant_jws = sign_jws(&wrong_signer, "pubky-grant", &raw_grant);

        let err = service.create_grant_session(&bad_grant_jws, &pop_jws).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::InvalidGrant(_)));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn create_grant_session_nonce_replay() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let hs_pubkey = service.homeserver_public_key();

        // Create a shared PoP nonce for replay
        let now = Utc::now().timestamp() as u64;
        let grant_id = GrantId::generate();
        let shared_nonce = PopNonce::generate();

        let raw_grant = GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: grant_id.clone(),
            iat: now,
            exp: now + 3600,
        };
        let grant_jws = sign_jws(&user_kp, "pubky-grant", &raw_grant);

        let pop_claims = PopProofClaims {
            aud: hs_pubkey.clone(),
            gid: grant_id.clone(),
            nonce: shared_nonce.clone(),
            iat: now,
        };
        let pop_jws = sign_jws(&client_kp, "pubky-pop", &pop_claims);

        // First call succeeds
        service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();

        // Second call with same nonce fails
        let err = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::NonceReplay));
    }

    // ── signup_grant_session ────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn signup_grant_session_open_mode() {
        let service = test_service().await;
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service
            .signup_grant_session(&grant_jws, &pop_jws, &SignupMode::Open, None)
            .await
            .unwrap();
        assert_eq!(response.session.pubky, user_kp.public_key());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn signup_grant_session_user_already_exists() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let err = service
            .signup_grant_session(&grant_jws, &pop_jws, &SignupMode::Open, None)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthServiceError::UserAlreadyExists));
    }

    // ── revoke_grant + resolve_grant_session ───────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn resolve_grant_session_happy_path() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, raw_grant) =
            sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();

        // Verify the minted JWT and resolve
        let jwt_compact = JwsCompact::parse(&response.token).unwrap();
        let jwt = verify_access_jwt(&jwt_compact, &service.homeserver_public_key()).unwrap();
        let session = service.resolve_grant_session(&jwt).await.unwrap();

        assert_eq!(session.user_key, user_kp.public_key());
        assert_eq!(session.grant_id, raw_grant.jti);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn resolve_grant_session_after_revoke() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, raw_grant) =
            sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();
        service.revoke_grant(&raw_grant.jti).await.unwrap();

        let jwt_compact = JwsCompact::parse(&response.token).unwrap();
        let jwt = verify_access_jwt(&jwt_compact, &service.homeserver_public_key()).unwrap();
        let err = service.resolve_grant_session(&jwt).await.unwrap_err();
        // Session was deleted by revoke_grant, so SessionNotFound (or GrantRevoked depending on timing)
        assert!(matches!(err, AuthServiceError::SessionNotFound | AuthServiceError::GrantRevoked));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn resolve_grant_session_missing_session() {
        let service = test_service().await;
        let hs_kp_pub = service.homeserver_public_key();

        // Create a valid JWT that points to a non-existent session
        let user_kp = Keypair::random();
        let now = Utc::now().timestamp() as u64;
        let claims = AccessJwtClaims {
            iss: hs_kp_pub.clone(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: now,
            exp: now + 3600,
        };
        let token = mint_access_jwt(&service.homeserver_keypair, &claims);
        let jwt = verify_access_jwt(&token, &hs_kp_pub).unwrap();

        let err = service.resolve_grant_session(&jwt).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::SessionNotFound));
    }

    // ── get_grant_session_info ─────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn get_grant_session_info_happy_path() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();
        let jwt_compact = JwsCompact::parse(&response.token).unwrap();
        let jwt = verify_access_jwt(&jwt_compact, &service.homeserver_public_key()).unwrap();
        let session = service.resolve_grant_session(&jwt).await.unwrap();

        let info = service.get_grant_session_info(&session).await.unwrap();
        assert_eq!(info.pubky, user_kp.public_key());
        assert_eq!(info.homeserver, service.homeserver_public_key());
    }

    // ── revoke_user_grant ─────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn revoke_user_grant_ownership_mismatch() {
        let service = test_service().await;
        let (user_a_kp, _) = create_test_user(&service).await;
        let (user_b_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();

        // User A creates a grant session
        let (grant_jws, pop_jws, raw_grant) =
            sign_grant(&user_a_kp, &client_kp, &service.homeserver_public_key());
        service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();

        // User B tries to revoke User A's grant
        let client_b_kp = Keypair::random();
        let (grant_b_jws, pop_b_jws, _) =
            sign_grant(&user_b_kp, &client_b_kp, &service.homeserver_public_key());
        let response_b = service.create_grant_session(&grant_b_jws, &pop_b_jws).await.unwrap();
        let jwt_b = JwsCompact::parse(&response_b.token).unwrap();
        let claims_b = verify_access_jwt(&jwt_b, &service.homeserver_public_key()).unwrap();
        let session_b = service.resolve_grant_session(&claims_b).await.unwrap();
        let auth_b = AuthSession::Grant(session_b);

        let err = service.revoke_user_grant(&raw_grant.jti, &auth_b).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::GrantOwnershipMismatch));
    }

    // ── list_active_grants ──────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn list_active_grants_happy_path() {
        let service = test_service().await;
        let (user_kp, user_id) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();

        let grants = service.list_active_grants(user_id).await.unwrap();
        assert_eq!(grants.len(), 1);
    }

    // ── signout_grant_session ──────────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn signout_grant_session_revokes_grant() {
        let service = test_service().await;
        let (user_kp, _) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();
        let jwt_compact = JwsCompact::parse(&response.token).unwrap();
        let jwt = verify_access_jwt(&jwt_compact, &service.homeserver_public_key()).unwrap();
        let session = service.resolve_grant_session(&jwt).await.unwrap();

        service.signout_grant_session(&session).await.unwrap();

        // Session should be gone
        let err = service.resolve_grant_session(&jwt).await.unwrap_err();
        assert!(matches!(err, AuthServiceError::SessionNotFound | AuthServiceError::GrantRevoked));
    }

    // ── resolve_user_id ─────────────────────────────────────────────

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn resolve_user_id_grant_session() {
        let service = test_service().await;
        let (user_kp, user_id) = create_test_user(&service).await;
        let client_kp = Keypair::random();
        let (grant_jws, pop_jws, _) = sign_grant(&user_kp, &client_kp, &service.homeserver_public_key());

        let response = service.create_grant_session(&grant_jws, &pop_jws).await.unwrap();
        let jwt_compact = JwsCompact::parse(&response.token).unwrap();
        let jwt = verify_access_jwt(&jwt_compact, &service.homeserver_public_key()).unwrap();
        let session = service.resolve_grant_session(&jwt).await.unwrap();

        let resolved = service.resolve_user_id(&AuthSession::Grant(session)).await.unwrap();
        assert_eq!(resolved, user_id);
    }

    // ── require_root_capability ─────────────────────────────────────

    #[test]
    fn require_root_capability_passes_with_root() {
        let session = GrantSession {
            user_key: Keypair::random().public_key(),
            capabilities: Capabilities::builder().cap(Capability::root()).finish(),
            grant_id: GrantId::generate(),
            token_id: TokenId::generate(),
            token_expires_at: 0,
        };
        assert!(AuthService::require_root_capability(&AuthSession::Grant(session)).is_ok());
    }

    #[test]
    fn require_root_capability_fails_without_root() {
        let session = GrantSession {
            user_key: Keypair::random().public_key(),
            capabilities: Capabilities::builder().cap(Capability::read("/")).finish(),
            grant_id: GrantId::generate(),
            token_id: TokenId::generate(),
            token_expires_at: 0,
        };
        let err = AuthService::require_root_capability(&AuthSession::Grant(session)).unwrap_err();
        assert!(matches!(err, AuthServiceError::RootCapabilityRequired));
    }
}

fn build_session_response(
    token: String,
    grant: &GrantClaims,
    homeserver: PublicKey,
    jwt_exp: u64,
    now: u64,
) -> GrantSessionResponse {
    GrantSessionResponse {
        token,
        session: GrantSessionInfo {
            homeserver,
            pubky: grant.iss.clone(),
            client_id: grant.client_id.clone(),
            capabilities: grant.caps.clone(),
            grant_id: grant.jti.clone(),
            token_expires_at: jwt_exp,
            grant_expires_at: grant.exp,
            created_at: now,
        },
    }
}
