use std::path::Path;

use pkarr::PublicKey;
use pubky_common::session::SessionInfo;

use super::core::PubkySession;
use crate::{
    Capabilities, PubkyHttpClient, Result, cross_log,
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
        let public_key = self.info().public_key().to_string();
        let cookie = self.cookie.clone();
        cross_log!(info, "Exporting session secret for {}", public_key);
        format!("{public_key}:{cookie}")
    }

    /// Rehydrate a session from a compact secret token `<pubkey>:<cookie_secret>`.
    ///
    /// Useful for scripts that need restarting. Helps avoiding a new Auth flow
    /// from a signer on a script restart.
    ///
    /// Performs a `/session` roundtrip to validate and hydrate the authoritative `SessionInfo`.
    /// Returns `AuthError::RequestExpired` if the cookie is invalid/expired.
    pub async fn import_secret(token: &str, client: Option<PubkyHttpClient>) -> Result<Self> {
        // 1) Get the transport for this session
        let client = match client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        // 2) Parse `<pubkey>:<cookie_secret>` (cookie may contain `:`, so split at the first one)
        let (pk_str, cookie) = token
            .split_once(':')
            .ok_or_else(|| RequestError::Validation {
                message: "invalid secret: expected `<pubkey>:<cookie>`".into(),
            })?;

        let public_key = PublicKey::try_from(pk_str).map_err(|_| RequestError::Validation {
            message: "invalid public key".into(),
        })?;
        cross_log!(info, "Importing session secret for {}", public_key);

        // 3) Build minimal session; placeholder SessionInfo will be replaced after validation.
        let placeholder = SessionInfo::new(&public_key, Capabilities::default(), None);
        let mut session = Self {
            client,
            info: placeholder,
            cookie: cookie.to_string(),
        };

        // 4) Validate cookie and fetch authoritative SessionInfo
        let info = session
            .revalidate()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        session.info = info;
        cross_log!(
            info,
            "Successfully imported session secret for {}",
            public_key
        );

        Ok(session)
    }

    /// Write the session secret token to disk. Ensures a `.sess` extension.
    ///
    /// Behavior:
    /// - If `secret_file_path` already ends with `.sess`, it is used as-is.
    /// - If it has no extension, `.sess` is added.
    /// - If it has a different extension, `.<ext>.sess` is appended (e.g., `foo.txt.sess`).
    ///
    /// On Unix, permissions are set to `0o600`.
    pub fn write_secret_file<P: AsRef<Path>>(&self, secret_file_path: P) -> std::io::Result<()> {
        let token = self.export_secret();
        let p = secret_file_path.as_ref();

        let target = match p.extension().and_then(|e| e.to_str()) {
            Some("sess") => p.to_path_buf(),
            Some(_) => {
                // Append, do not replace: `name.ext` -> `name.ext.sess`
                let mut out = p.to_path_buf();
                let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("session");
                out.set_file_name(format!("{fname}.sess"));
                out
            }
            None => {
                // No extension: add `.sess`
                let mut out = p.to_path_buf();
                out.set_extension("sess");
                out
            }
        };

        std::fs::write(&target, token)?;
        cross_log!(
            info,
            "Wrote session secret for {} to {}",
            self.info().public_key(),
            target.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))?;
        };
        Ok(())
    }

    /// Restore a session from a secret token stored in a file. Requires a `.sess` extension.
    ///
    /// Validation:
    /// - `.sess` — valid; file is read and parsed.
    /// - `.pkarr` — rejected with a clear error message pointing to `Keypair::from_secret_file`.
    /// - Any other or missing extension — rejected with a `.sess`-specific error.
    pub async fn from_secret_file(
        secret_file_path: &Path,
        client: Option<PubkyHttpClient>,
    ) -> Result<Self> {
        match secret_file_path.extension().and_then(|e| e.to_str()) {
            Some("sess") => { /* ok */ }
            Some("pkarr") => {
                return Err(RequestError::Validation {
                    message: format!(
                        "refused to load `{}`: `.pkarr` is a keypair secret. \
                         Use `Keypair::from_secret_file` to load keys. \
                         Session secrets must use the `.sess` extension.",
                        secret_file_path.display()
                    ),
                }
                .into());
            }
            Some(other) => {
                return Err(RequestError::Validation {
                    message: format!(
                        "invalid session secret extension `.{other}` for `{}`; expected `.sess`",
                        secret_file_path.display()
                    ),
                }
                .into());
            }
            None => {
                return Err(RequestError::Validation {
                    message: format!(
                        "missing extension for `{}`; session secret files must end with `.sess`",
                        secret_file_path.display()
                    ),
                }
                .into());
            }
        }

        let token =
            std::fs::read_to_string(secret_file_path).map_err(|e| RequestError::Validation {
                message: format!("failed to read session secret file: {e}"),
            })?;
        cross_log!(
            info,
            "Loading session secret from {}",
            secret_file_path.display()
        );
        Self::import_secret(token.trim(), client).await
    }
}
