use pkarr::PublicKey;
use pubky_common::capabilities::Capabilities;

use super::core::PubkyAgent;
use crate::{
    PubkyClient, Result, Session,
    errors::{AuthError, RequestError},
};

impl PubkyAgent {
    /// Export the minimum needed to restore this agent later (native only).
    /// Returns a single compact secret token `<pubkey>:<cookie_secret>`
    ///
    /// Useful for scripts that need restarting. Helps avoiding a new Auth flow
    /// from a signer on a script restart.
    ///
    /// Treat the returned String as a **bearer secret**. Do not log it; store it
    /// securely.
    pub fn export_secret(&self) -> String {
        let public_key = self.public_key().to_string();
        let cookie = self.cookie.clone();
        format!("{public_key}:{cookie}")
    }

    /// Rehydrate an agent from a compact secret token `<pubkey>:<cookie_secret>` (native only).
    ///
    /// Useful for scripts that need restarting. Helps avoiding a new Auth flow
    /// from a signer on a script restart. For example you could read this secret from an `.env`
    /// file
    ///
    /// Performs a `/session` roundtrip to validate and hydrate the authoritative `Session`.
    /// Returns `AuthError::RequestExpired` if the cookie is invalid/expired.
    pub async fn import_secret(client: &PubkyClient, token: &str) -> Result<Self> {
        // 1) Parse `<pubkey>:<cookie_secret>` (cookie may contain `:`, so split at the first one)
        let (pk_str, cookie) = token
            .split_once(':')
            .ok_or_else(|| RequestError::Validation {
                message: "invalid secret: expected `<pubkey>:<cookie>`".into(),
            })?;

        let public_key = PublicKey::try_from(pk_str).map_err(|_| RequestError::Validation {
            message: "invalid public key".into(),
        })?;

        // 2) Build minimal agent; placeholder Session will be replaced after validation.
        let placeholder = Session::new(&public_key, Capabilities::default(), None);
        let mut agent = PubkyAgent {
            client: client.clone(),
            session: placeholder,
            cookie: cookie.to_string(),
        };

        // 3) Validate cookie and fetch authoritative Session
        let session = agent
            .session_from_homeserver()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        agent.session = session;

        Ok(agent)
    }
}
