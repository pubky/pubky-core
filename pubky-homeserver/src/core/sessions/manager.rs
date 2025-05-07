use std::time::Duration;
use pkarr::PublicKey;
use pubky_common::{capabilities::Capability, session::Session};
use tower_cookies::Cookies;
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
        let jwt = self.jwt_service.create_token(user_pubkey, capabilities, expires_after, session_id.to_string())?;
        Ok((session_id, session, jwt))
    }

    /// Extracts the session ID from the cookie(s).
    /// Tries both legacy cookie and simple JWT.
    /// 
    /// Only if the `user_pubkey` is supplied, it will try to extract the session ID from the legacy cookie.
    /// That's because the legacy cookie was really badly designed 💀.
    pub fn extract_session_id_from_cookies(&self, cookies: &Cookies, user_pubkey: Option<&PublicKey>) -> Option<SessionId> {
        if let Some(user_pubkey) = user_pubkey {
            let session_id = self.extract_session_id_from_legacy_cookie(cookies, user_pubkey);
            if session_id.is_some() {
                return session_id;
            };
        };

        self.extract_session_id_from_jwt(cookies)
    }

    /// Extracts the session ID from the legacy cookie.
    fn extract_session_id_from_legacy_cookie(&self, cookies: &Cookies, user_pubkey: &PublicKey) -> Option<SessionId> {
        cookies
            .get(&user_pubkey.to_string())
            .map(|c| c.value().to_string())
            .map(|session_id| SessionId(session_id))
    }

    /// Extracts the session ID from the JWT token.
    fn extract_session_id_from_jwt(&self, cookies: &Cookies) -> Option<SessionId> {
        let cookie = cookies.get("auth_token")?;
        let jwt = match JwtToken::new(cookie.value().to_string()) {
            Ok(jwt) => jwt,
            Err(e) => {
                tracing::debug!("Error parsing user session JWT: {}", e);
                return None;
            },
        };
        let session_id = jwt.decoded().claims.jti.clone();
        Some(SessionId(session_id))
    }

}
