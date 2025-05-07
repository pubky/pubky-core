use std::time::Duration;
use pkarr::PublicKey;
use pubky_common::{capabilities::Capability, session::Session};
use crate::{persistence::lmdb::{tables::sessions::SessionId, LmDB}, AppContext};
use super::{JwtService, JwtToken};


/// Manages all of our different session types.
/// 
/// We have:
/// - Legacy cookie sessions
/// - Simple JWT sessions
/// 
#[derive(Clone, Debug)]
pub(crate) struct SessionManager {
    jwt_service: JwtService,
    db: LmDB,
}

impl SessionManager {
    pub fn new(context: &AppContext) -> Self {
        Self {
            jwt_service: JwtService::new(&context.keypair).unwrap(),
            db: context.db.clone(),
        }
    }

    /// Creates a new session in the database and returns the session ID and the JWT token.
    pub fn create_session(
        &self,
        user_pubkey: &PublicKey,
        capabilities: &[Capability],
    ) -> anyhow::Result<(SessionId, Session, JwtToken)> {
        let (session_id, session) = self.db.create_session(user_pubkey, capabilities)?;
        let one_day = Duration::from_secs(86400);
        let expires_after = one_day * 365;
        let jwt = self.jwt_service.create_token(user_pubkey, capabilities, expires_after, Some(session_id.to_string()))?;
        Ok((session_id, session, jwt))
    }

}
