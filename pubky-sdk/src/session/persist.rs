use pkarr::PublicKey;
use pubky_common::{capabilities::Capabilities, session::SessionInfo};

use super::core::PubkySession;
use crate::{
    PubkyHttpClient, Result,
    errors::{AuthError, RequestError},
};

impl PubkySession {
    /// Export the minimum data needed to restore this session later.
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

    /// Rehydrate a session from a compact secret token `<pubkey>:<cookie_secret>`.
    ///
    /// Useful for scripts that need restarting. Helps avoiding a new Auth flow
    /// from a signer on a script restart.
    ///
    /// Performs a `/session` roundtrip to validate and hydrate the authoritative `SessionInfo`.
    /// Returns `AuthError::RequestExpired` if the cookie is invalid/expired.
    pub async fn import_secret(client: &PubkyHttpClient, token: &str) -> Result<Self> {
        // 1) Parse `<pubkey>:<cookie_secret>` (cookie may contain `:`, so split at the first one)
        let (pk_str, cookie) = token
            .split_once(':')
            .ok_or_else(|| RequestError::Validation {
                message: "invalid secret: expected `<pubkey>:<cookie>`".into(),
            })?;

        let public_key = PublicKey::try_from(pk_str).map_err(|_| RequestError::Validation {
            message: "invalid public key".into(),
        })?;

        // 2) Build minimal session; placeholder SessionInfo will be replaced after validation.
        let placeholder = SessionInfo::new(&public_key, Capabilities::default(), None);
        let mut session = PubkySession {
            client: client.clone(),
            info: placeholder,
            cookie: cookie.to_string(),
        };

        // 3) Validate cookie and fetch authoritative SessionInfo
        let info = session
            .revalidate_session()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        session.info = info;

        Ok(session)
    }
}
