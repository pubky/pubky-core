//! Cookie auth service — orchestrates deprecated cookie auth use cases.

use pubky_common::{
    auth::AuthToken, capabilities::Capabilities, crypto::PublicKey, session::CookieSessionRecord,
};

use crate::persistence::sql::{signup_code::SignupCode, user::UserRepository, SqlDb};
use crate::shared::{HttpError, HttpResult};

use super::persistence::{SessionEntity, SessionRepository, SessionSecret};
use super::verifier::CookieAuthVerifier;
use crate::client_server::auth::{AuthSession, SignupService};

#[derive(Clone, Debug)]
pub(crate) struct CookieAuthService {
    sql_db: SqlDb,
    verifier: CookieAuthVerifier,
    signup_service: SignupService,
}

impl CookieAuthService {
    pub(crate) fn new(
        sql_db: SqlDb,
        verifier: CookieAuthVerifier,
        signup_service: SignupService,
    ) -> Self {
        Self {
            sql_db,
            verifier,
            signup_service,
        }
    }

    pub(crate) async fn signup(
        &self,
        body: &[u8],
        signup_token: Option<&SignupCode>,
    ) -> HttpResult<CookieSessionCreation> {
        let token = self.verify(body)?;
        let user = self
            .signup_service
            .create_new_user(token.public_key(), signup_token)
            .await?;
        let session_secret = self.create_session(user.id, token.capabilities()).await?;

        Ok(CookieSessionCreation {
            public_key: user.public_key,
            session_secret,
            capabilities: token.capabilities().clone(),
        })
    }

    pub(crate) async fn signin(&self, body: &[u8]) -> HttpResult<CookieSessionCreation> {
        let token = self.verify(body)?;
        let public_key = token.public_key();
        let user = UserRepository::get(public_key, &mut self.sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => HttpError::not_found(),
                e => e.into(),
            })?;
        let session_secret = self.create_session(user.id, token.capabilities()).await?;

        Ok(CookieSessionCreation {
            public_key: user.public_key,
            session_secret,
            capabilities: token.capabilities().clone(),
        })
    }

    pub(crate) async fn resolve_session_from_cookie(
        &self,
        cookie_value: Option<String>,
        public_key: &PublicKey,
    ) -> Option<AuthSession> {
        let session_secret = SessionSecret::new(cookie_value?).ok()?;
        let session = self.get_session(&session_secret).await.ok()?;

        if &session.user_pubkey != public_key {
            return None;
        }

        Some(AuthSession::Cookie(session))
    }

    pub(crate) async fn signout(&self, auth: Option<AuthSession>) -> HttpResult<()> {
        if let Some(AuthSession::Cookie(cookie_session)) = auth {
            SessionRepository::delete(&cookie_session.secret, &mut self.sql_db.pool().into())
                .await?;
        }

        Ok(())
    }

    fn verify(&self, body: &[u8]) -> Result<AuthToken, pubky_common::auth::Error> {
        self.verifier.verify(body)
    }

    async fn create_session(
        &self,
        user_id: i32,
        capabilities: &Capabilities,
    ) -> Result<SessionSecret, sqlx::Error> {
        SessionRepository::create(user_id, capabilities, &mut self.sql_db.pool().into()).await
    }

    async fn get_session(&self, secret: &SessionSecret) -> Result<SessionEntity, sqlx::Error> {
        SessionRepository::get_by_secret(secret, &mut self.sql_db.pool().into()).await
    }
}

pub(crate) struct CookieSessionCreation {
    pub(crate) public_key: PublicKey,
    pub(crate) session_secret: SessionSecret,
    pub(crate) capabilities: Capabilities,
}

impl CookieSessionCreation {
    pub(crate) fn to_record(&self) -> CookieSessionRecord {
        CookieSessionRecord::new(&self.public_key, self.capabilities.clone(), None)
    }
}
