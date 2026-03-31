//! Cookie-based authentication logic.
//!
//! Extracts and validates deprecated session cookies.

use pubky_common::crypto::PublicKey;
use tower_cookies::Cookies;

use super::persistence::{SessionEntity, SessionRepository, SessionSecret};
use crate::client_server::auth::AuthSession;
use crate::client_server::auth::AuthState;

/// Deprecated cookie-based session data.
#[derive(Clone, Debug)]
pub struct CookieSession {
    /// The session entity from the database.
    pub session: SessionEntity,
}

/// Get the session secret from the cookies.
/// Returns `None` if the session secret is not found or invalid.
pub fn session_secret_from_cookies(
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    let value = cookies
        .get(&public_key.z32())
        .map(|c| c.value().to_string())?;
    SessionSecret::new(value).ok()
}

/// Authenticate via deprecated session cookie.
pub async fn authenticate_cookie(
    state: &AuthState,
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<AuthSession> {
    let session_secret = session_secret_from_cookies(cookies, public_key)?;

    let session =
        SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into())
            .await
            .ok()?;

    if &session.user_pubkey != public_key {
        return None;
    }

    Some(AuthSession::Cookie(CookieSession { session }))
}
