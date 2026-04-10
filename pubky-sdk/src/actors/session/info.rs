//! Minimal, auth-agnostic session metadata.

use pubky_common::{capabilities::Capability, crypto::PublicKey};

/// Minimal, auth-agnostic session metadata.
///
/// Carries only the fields that every credential type can produce and that
/// callers actually consume: the user's public key and the capabilities
/// granted to the session.
///
/// Credential-specific details live behind the capability views:
/// - JWT: [`GrantSessionInfo`](pubky_common::auth::grant_session::GrantSessionInfo)
///   via `session.as_jwt().session_info()`
/// - Cookie: [`CookieSessionRecord`](pubky_common::session::CookieSessionRecord)
///   via `session.as_cookie().session_info()`
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionInfo {
    public_key: PublicKey,
    capabilities: Vec<Capability>,
}

impl SessionInfo {
    /// Create a new minimal session info.
    pub fn new(public_key: PublicKey, capabilities: Vec<Capability>) -> Self {
        Self {
            public_key,
            capabilities,
        }
    }

    /// Returns the public key this session authorizes for.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Returns the capabilities this session provides.
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}
