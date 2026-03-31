//! Shared authentication session type.
//!
//! [`AuthSession`] is the unified enum that bridges cookie-based and JWT-based
//! authentication. It is inserted into request extensions by the authentication
//! middleware and extracted by route handlers.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

use super::cookie::auth::CookieSession;
use super::jwt::auth::BearerSession;

/// Resolved authentication context — inserted into request extensions by the
/// authentication middleware. Handlers just add `auth: AuthSession` as a parameter.
#[derive(Clone, Debug)]
pub enum AuthSession {
    /// Deprecated cookie-based session.
    Cookie(CookieSession),
    /// Grant-based JWT Bearer token session.
    Bearer(BearerSession),
}

impl AuthSession {
    /// Capabilities regardless of auth method.
    pub fn capabilities(&self) -> &Capabilities {
        match self {
            AuthSession::Cookie(c) => &c.session.capabilities,
            AuthSession::Bearer(b) => &b.capabilities,
        }
    }

    /// User public key regardless of auth method.
    pub fn user_key(&self) -> &PublicKey {
        match self {
            AuthSession::Cookie(c) => &c.session.user_pubkey,
            AuthSession::Bearer(b) => &b.user_key,
        }
    }
}

impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthSession>()
            .cloned()
            .ok_or((
                StatusCode::UNAUTHORIZED,
                "No authenticated session found",
            ))
            .map_err(|e| e.into_response())
    }
}
