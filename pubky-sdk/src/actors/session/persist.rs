//! Cookie session persistence — `import_secret` / `from_secret_file`.
//!
//! These constructors live on [`PubkySession`] (rather than on a cookie
//! view) because they *create* a session from stored material rather than
//! acting on an existing one. The cookie-only post-construction operations
//! (`export_secret`, `write_secret_file`) live on
//! [`super::view::CookieSessionView`].
//!
//! ## Cross-target availability
//!
//! - [`PubkySession::import_secret`] — string-based, **cross-target**. Works
//!   on native, on Node.js WASM, and on browser WASM (though browser WASM
//!   typically cannot obtain the secret in the first place because the
//!   fetch spec hides `Set-Cookie`).
//! - [`PubkySession::from_secret_file`] — filesystem-based, **native-only**.
//!
//! When the cookie credential is retired, this entire file deletes
//! alongside [`super::credential::cookie`].

use std::sync::Arc;

use crate::PublicKey;
use pubky_common::{capabilities::Capabilities, session::SessionInfo};

use super::core::PubkySession;
use super::credential::{CookieCredential, SessionCredential};
use crate::{
    PubkyHttpClient, Result, cross_log,
    errors::{AuthError, RequestError},
};

impl PubkySession {
    /// Rehydrate a session from a compact secret token `<pubkey>:<cookie_secret>`.
    ///
    /// Useful for scripts that need restarting. Helps avoid a new auth flow
    /// from a signer on a script restart.
    ///
    /// Performs a `/session` roundtrip to validate and hydrate the
    /// authoritative `SessionInfo`. Returns [`AuthError::RequestExpired`]
    /// if the cookie is invalid/expired.
    ///
    /// Available on every target.
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] if the token
    ///   is malformed or contains an invalid public key.
    /// - Propagates transport failures while validating the session with
    ///   the homeserver.
    pub async fn import_secret(token: &str, client: Option<PubkyHttpClient>) -> Result<Self> {
        let client = match client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        // Cookie may contain `:`, so split at the first colon only.
        let (pk_str, cookie) = token
            .split_once(':')
            .ok_or_else(|| RequestError::Validation {
                message: "invalid secret: expected `<pubkey>:<cookie>`".into(),
            })?;

        let public_key =
            PublicKey::try_from_z32(pk_str).map_err(|_err| RequestError::Validation {
                message: "invalid public key".into(),
            })?;
        cross_log!(info, "Importing session secret for {}", public_key);

        // Build minimal session; placeholder SessionInfo will be replaced
        // after validation.
        let placeholder = SessionInfo::new(&public_key, Capabilities::default(), None);
        let cookie_credential = CookieCredential::new(
            public_key.clone(),
            Some(cookie.to_string()),
            placeholder,
        );
        let credential: Arc<dyn SessionCredential> = Arc::new(cookie_credential);
        let session = Self::from_credential(client, Arc::clone(&credential));

        // Validate cookie and fetch authoritative SessionInfo. The
        // credential is statically a CookieCredential, so as_cookie()
        // always returns Some.
        let info = session
            .revalidate()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        if let Some(c) = credential.as_cookie() {
            c.replace_info(info);
        }
        cross_log!(
            info,
            "Successfully imported session secret for {}",
            public_key
        );

        Ok(session)
    }

    /// Restore a session from a secret token stored in a file. Requires a
    /// `.sess` extension. Native-only — depends on the standard filesystem
    /// APIs.
    ///
    /// Validation:
    /// - `.sess` — valid; file is read and parsed.
    /// - `.pkarr` — rejected with a clear error message pointing to
    ///   `Keypair::from_secret_file`.
    /// - Any other or missing extension — rejected with a `.sess`-specific
    ///   error.
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] when the file
    ///   extension is not `.sess`.
    /// - Returns [`crate::errors::RequestError::Validation`] if the file
    ///   cannot be read.
    /// - Propagates errors from [`Self::import_secret`] when the stored
    ///   token is invalid or when the session cannot be revalidated.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_secret_file(
        secret_file_path: &std::path::Path,
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
