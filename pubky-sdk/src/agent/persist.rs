use pkarr::PublicKey;
use pubky_common::capabilities::Capabilities;

use super::core::PubkyAgent;
use crate::{PubkyClient, Result, Session, errors::AuthError};

impl PubkyAgent {
    /// Export the minimum needed to restore this agent later (native only).
    ///
    /// Returns `(public_key, cookie)`. Treat the cookie as a **bearer secret**.
    /// Do not log it; store it securely (OS keychain, env-injected at runtime, etc.).
    pub fn export_secret(&self) -> (PublicKey, String) {
        (self.public_key().clone(), self.cookie.clone())
    }

    /// Rehydrate an agent from `(public_key, secret)` (native only).
    ///
    /// This constructs a temporary agent and performs a `/session` roundtrip to
    /// obtain the authoritative `Session`. Fails with `AuthError::RequestExpired`
    /// if the cookie is invalid/expired.
    pub async fn import_secret(
        client: &PubkyClient,
        (public_key, cookie): (PublicKey, String),
    ) -> Result<Self> {
        // 1) Build a minimal agent; the placeholder Session is replaced after validation.
        let placeholder_session = Session::new(&public_key, Capabilities::default(), None);
        let mut agent = PubkyAgent {
            client: client.clone(),
            session: placeholder_session,
            cookie,
        };

        // 2) Validate secret and get a Session
        let session = agent
            .session_from_homeserver()
            .await?
            .ok_or(AuthError::RequestExpired)?;

        agent.session = session;

        Ok(agent)
    }
}
