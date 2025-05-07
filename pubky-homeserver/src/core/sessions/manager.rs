use std::{str::FromStr, time::Duration};
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
        let jwt = self.jwt_service.create_token(user_pubkey, capabilities, expires_after, session_id.clone())?;
        Ok((session_id, session, jwt))
    }

    /// Extracts the session ID from the cookie(s).
    /// Tries both legacy cookie and simple JWT.
    pub fn extract_session_id_from_cookies(&self, cookies: &Cookies) -> Option<SessionId> {
        if let Some(session_id) = self.extract_session_id_from_legacy_cookie(cookies) {
            return Some(session_id);
        };

        self.extract_session_id_from_jwt(cookies)
    }

    /// Extracts the session from the cookie(s).
    pub fn extract_session_from_cookies(&self, cookies: &Cookies) -> Option<Session> {
        let session_id = self.extract_session_id_from_cookies(cookies)?;
        self.db.get_session(&session_id).ok()?
    }

    /// Extracts the session ID from the legacy cookie.
    /// The cookie name is the user's public key.
    fn extract_session_id_from_legacy_cookie(&self, cookies: &Cookies) -> Option<SessionId> {
        // Find a cookie that is a valid public key
        // This is a bit of a hack, because of the initial design of the cookie
        // It works though because usually cookies are not in our public key format
        let cookie_list = cookies.list();
        let (_user_pubkey, value) = cookie_list.iter().find_map(|c| {
            let name = c.name();
            match PublicKey::from_str(name) {
                Ok(pubkey) => {
                    Some((pubkey, c.value().to_string()))
                },
                Err(_) => None, // Failed to parse as public key, ignore
            }
        })?;

        // Check if the value is a valid session ID
        // Legacy cookie content.
        if let Ok(session_id) = SessionId::new(&value) {
            return Some(session_id);
        }

        // Check if the value is a valid JWT token
        // New JWT cookie content.
        if let Ok(jwt) = JwtToken::new(value) {
            return Some(jwt.decoded().claims.jti.clone());
        }

        None
    }

    /// Extracts the session ID from the JWT token.
    /// The cookie name is `auth_token`.
    fn extract_session_id_from_jwt(&self, cookies: &Cookies) -> Option<SessionId> {
        let cookie = cookies.get("auth_token")?;
        let jwt = match JwtToken::new(cookie.value().to_string()) {
            Ok(jwt) => jwt,
            Err(e) => {
                tracing::debug!("Error parsing user session JWT: {}", e);
                return None;
            },
        };
        Some(jwt.decoded().claims.jti.clone())
    }

}
