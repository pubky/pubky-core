//! Shared authentication session type.
//!
//! [`AuthSession`] is the unified enum that bridges cookie-based and JWT-based
//! authentication. It is inserted into request extensions by the authentication
//! middleware and extracted by route handlers.

use pubky_common::capabilities::Capabilities;
use pubky_common::crypto::PublicKey;

use super::cookie::persistence::SessionEntity;
use super::jwt::auth::BearerSession;

/// Resolved authentication context — inserted into request extensions by the
/// authentication middleware. Handlers just add `auth: AuthSession` as a parameter.
#[derive(Clone, Debug)]
pub enum AuthSession {
    /// Deprecated cookie-based session.
    Cookie(SessionEntity),
    /// Grant-based JWT Bearer token session.
    Bearer(BearerSession),
}

impl AuthSession {
    /// Capabilities regardless of auth method.
    pub fn capabilities(&self) -> &Capabilities {
        match self {
            AuthSession::Cookie(c) => &c.capabilities,
            AuthSession::Bearer(b) => &b.capabilities,
        }
    }

    /// User public key regardless of auth method.
    pub fn user_key(&self) -> &PublicKey {
        match self {
            AuthSession::Cookie(c) => &c.user_pubkey,
            AuthSession::Bearer(b) => &b.user_key,
        }
    }
}

