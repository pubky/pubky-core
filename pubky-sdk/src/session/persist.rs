use std::path::Path;

use pkarr::PublicKey;
use pubky_common::session::SessionInfo;

use super::core::PubkySession;
use crate::{
    Capabilities, Result,
    errors::{AuthError, RequestError},
    global_client,
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
    pub async fn import_secret(token: &str) -> Result<Self> {
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
            client: global_client()?,
            info: placeholder,
            cookie: cookie.to_string(),
        };

        // 3) Validate cookie and fetch authoritative SessionInfo
        let info = session
            .revalidate()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        session.info = info;

        Ok(session)
    }

    /// Write the session secret token to a file as plain text: `<pubkey>:<cookie_secret>`.
    /// If the file exists, it is overwritten. On Unix, permissions are set to 600.
    pub fn write_secret_file(&self, secret_file_path: &Path) -> std::io::Result<()> {
        let token = self.export_secret();
        std::fs::write(secret_file_path, token)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(secret_file_path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Restore a session from a secret token stored in a file.
    /// Reads the file, trims whitespace, then calls `import_secret` to validate and hydrate.
    pub async fn from_secret_file(secret_file_path: &Path) -> Result<Self> {
        let token =
            std::fs::read_to_string(secret_file_path).map_err(|e| RequestError::Validation {
                message: format!("failed to read session secret file: {e}"),
            })?;
        Self::import_secret(token.trim()).await
    }
}
