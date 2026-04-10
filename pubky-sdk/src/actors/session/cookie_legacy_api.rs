//! Legacy cookie-session API shim on [`PubkySession`](super::core::PubkySession).
//!
//! This module exists only to preserve the older convenience methods on
//! `PubkySession` itself, such as `export`, `import`, `import_secret`,
//! `from_secret_file`, and `write_secret_file`.
//!
//! It does **not** implement cookie-session mechanics. The actual cookie
//! session construction and rehydration logic lives in [`super::cookie`], and
//! the underlying credential type lives in [`super::credential::cookie`].
//!
//! In short:
//! - [`super::cookie`] = cookie session implementation/bootstrap
//! - [`super::cookie_legacy_api`] = legacy `PubkySession` compatibility surface

use super::core::PubkySession;
use crate::{PubkyHttpClient, Result};

impl PubkySession {
    /// Export session metadata for rehydrating after a tab refresh or process restart.
    ///
    /// Delegates to [`super::view::CookieSessionView::export`]. Panics if the
    /// session is not cookie-backed.
    #[must_use]
    pub fn export(&self) -> String {
        self.as_cookie()
            .expect("export() is only valid for cookie sessions")
            .export()
    }

    /// Restore a session from an `export()` string.
    ///
    /// Delegates to [`super::cookie::import_session`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] if the export string is malformed.
    /// - On native, returns an error because exports are only supported on WASM.
    pub async fn import(export: &str, client: Option<PubkyHttpClient>) -> Result<Self> {
        super::cookie::import_session(export, client).await
    }

    /// Rehydrate a session from a compact secret token `<pubkey>:<cookie_secret>`.
    ///
    /// Delegates to [`super::cookie::import_session_secret`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] if the token is malformed.
    /// - Propagates transport failures while validating the session.
    pub async fn import_secret(token: &str, client: Option<PubkyHttpClient>) -> Result<Self> {
        super::cookie::import_session_secret(token, client).await
    }

    /// Restore a session from a secret token stored in a file.
    ///
    /// Delegates to [`super::cookie::session_from_secret_file`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] when the file extension is not `.sess`.
    /// - Propagates errors from the stored token validation.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn from_secret_file(
        path: &std::path::Path,
        client: Option<PubkyHttpClient>,
    ) -> Result<Self> {
        super::cookie::session_from_secret_file(path, client).await
    }

    /// Write the session secret token to disk. Ensures a `.sess` extension.
    ///
    /// Delegates to [`super::view::CookieSessionView::write_secret_file`].
    ///
    /// # Errors
    /// - Returns [`std::io::Error`] if the file cannot be written or
    ///   permissions cannot be set.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_secret_file<P: AsRef<std::path::Path>>(
        &self,
        secret_file_path: P,
    ) -> std::io::Result<()> {
        self.as_cookie()
            .expect("write_secret_file() is only valid for cookie sessions")
            .write_secret_file(secret_file_path)
    }
}
