//! Cookie-specific [`PubkySession`] constructors and persistence.
//!
//! This module is the cookie counterpart of [`super::jwt`]. It contains every
//! `impl PubkySession` method that depends on the legacy cookie flow:
//! construction from an [`AuthToken`], `Set-Cookie` header parsing, browser
//! WASM rehydration (`export` / `import`), and native secret persistence
//! (`import_secret` / `from_secret_file`).
//!
//! **Retirement plan:** When the cookie credential is retired, delete this
//! file alongside [`super::credential::cookie`] and
//! [`super::view::CookieSessionView`]. No edits to `core.rs` are required.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use pubky_common::{
    capabilities::Capabilities,
    crypto::PublicKey,
    session::SessionInfo,
};
use reqwest::Method;

use super::core::PubkySession;
use super::credential::{CookieCredential, SessionCredential};
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{AuthError, RequestError};
use crate::{
    AuthToken, PubkyHttpClient, Result, cross_log, util::check_http_status,
};

// =====================================================================
// Construction from AuthToken (legacy sign-in / sign-up)
// =====================================================================

impl PubkySession {
    /// Establish a session from a signed [`AuthToken`] (legacy cookie flow).
    ///
    /// POSTs the token to the homeserver's `/session` endpoint and constructs a
    /// cookie-based [`PubkySession`].
    pub(crate) async fn new(token: &AuthToken, client: PubkyHttpClient) -> Result<Self> {
        let url = format!("pubky{}/session", token.public_key().z32());
        cross_log!(
            info,
            "Establishing new session exchange for {}",
            token.public_key()
        );
        let resolved = resolve_pubky(&url)?;
        let response = client
            .cross_request(Method::POST, resolved)
            .await?
            .body(token.serialize())
            .send()
            .await?;

        let response = check_http_status(response).await?;
        cross_log!(
            info,
            "Session exchange for {} succeeded; constructing session",
            token.public_key()
        );
        Self::new_from_response(client, response).await
    }

    /// Construct a cookie-based session **from a successful `/session` or `/signup` response**.
    ///
    /// - Reads the `SessionInfo` body (to learn the user pubky).
    /// - Tries to capture the session cookie from the `Set-Cookie` headers
    ///   so it can be replayed manually. The capture succeeds on native and
    ///   on Node.js WASM. It fails on browser WASM, where the fetch spec
    ///   hides `Set-Cookie` from JavaScript — in that case the cookie is
    ///   stored as `None` and the browser cookie jar handles attachment.
    ///
    /// # Errors
    /// - On native we still treat a missing `Set-Cookie` as a hard error
    ///   because the runtime should always expose it.
    pub(crate) async fn new_from_response(
        client: PubkyHttpClient,
        response: reqwest::Response,
    ) -> Result<Self> {
        // Snapshot Set-Cookie headers before consuming the body. This is a
        // single code path on all targets — what differs by runtime is
        // whether the underlying fetch implementation surfaces the header
        // values to us.
        let raw_set_cookies = collect_set_cookies(&response);

        let bytes = response.bytes().await?;
        let info = SessionInfo::deserialize(&bytes)?;
        let user = info.public_key().clone();
        let cookie_name = user.z32();
        let cookie = raw_set_cookies
            .iter()
            .filter_map(|raw| cookie::Cookie::parse(raw.clone()).ok())
            .find(|c| c.name() == cookie_name)
            .map(|c| c.value().to_string());

        // Native always has access to Set-Cookie. Treat its absence as a
        // hard error so we never silently lose the secret on this target.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if cookie.is_none() {
                return Err(AuthError::Validation("missing session cookie".into()).into());
            }
        }

        #[cfg(target_arch = "wasm32")]
        if cookie.is_none() {
            cross_log!(
                info,
                "Hydrating WASM cookie session without captured secret \
                 (browser jar will handle attachment) for {}",
                user
            );
        }

        cross_log!(info, "Hydrated cookie session for {}", user);
        let credential = CookieCredential::new(user, cookie, info);
        Ok(Self::from_credential(client, Arc::new(credential)))
    }
}

// =====================================================================
// Browser WASM rehydration (export / import)
// =====================================================================

impl PubkySession {
    /// Export session metadata for rehydrating after a tab refresh or process restart.
    ///
    /// The returned string contains **no secrets**; it is a base64 encoding of the
    /// public `SessionInfo`. The caller remains responsible for persisting the
    /// HTTP-only session cookie; `export()` merely captures the metadata needed to
    /// reconstruct a `PubkySession` handle.
    #[must_use]
    pub fn export(&self) -> String {
        let info = self.info();
        cross_log!(info, "Exporting session for {}", info.public_key());
        STANDARD.encode(info.serialize())
    }

    /// Restore a session from an `export()` string. No secrets are read or written;
    /// the HTTP-only cookie jar must still contain the session cookie.
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] if the export string is malformed.
    /// - Returns [`crate::errors::AuthError::RequestExpired`] if the cookie is missing/expired.
    /// - Propagates transport failures while revalidating the session with the homeserver.
    #[cfg(target_arch = "wasm32")]
    pub async fn import(export: &str, client: Option<PubkyHttpClient>) -> Result<Self> {
        let client = match client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        let bytes = STANDARD
            .decode(export)
            .map_err(|e| RequestError::Validation {
                message: format!("invalid session export: {e}"),
            })?;
        let info = SessionInfo::deserialize(&bytes).map_err(|e| RequestError::Validation {
            message: format!("invalid session export: {e}"),
        })?;

        let user = info.public_key().clone();
        // Browser WASM cannot read Set-Cookie, so the secret is None and
        // attachment is delegated to the runtime cookie jar.
        let credential: Arc<dyn SessionCredential> =
            Arc::new(CookieCredential::new(user, None, info));
        let session = Self::from_credential(client, Arc::clone(&credential));
        let info = session
            .revalidate()
            .await?
            .ok_or(AuthError::RequestExpired)?;
        // We know the credential is a cookie credential — propagate the
        // server-authoritative info into its snapshot.
        if let Some(c) = credential.as_cookie() {
            c.replace_info(info);
        }
        cross_log!(info, "Rehydrated session");
        Ok(session)
    }

    /// Restore a session from an `export()` string (unsupported on native targets).
    ///
    /// Use [`Self::import_secret`] on native to restore a session using the secret token instead.
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] because exports are only supported on WASM.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(
        clippy::unused_async,
        reason = "keep async signature aligned with WASM build"
    )]
    pub async fn import(_export: &str, _client: Option<PubkyHttpClient>) -> Result<Self> {
        Err(RequestError::Validation {
            message: "session import is only supported on WASM targets".into(),
        }
        .into())
    }
}

// =====================================================================
// Native secret persistence (import_secret / from_secret_file)
// =====================================================================

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

// =====================================================================
// Helpers
// =====================================================================

/// Cross-target reader for `Set-Cookie` response header values.
///
/// `reqwest::HeaderMap::get_all` returns the same iterator on all targets,
/// but the *underlying* fetch implementation decides whether the header is
/// surfaced at all:
///
/// - **Native**: always returns every `Set-Cookie` value verbatim.
/// - **Node.js WASM** (undici): returns the values; the browser-only fetch
///   spec restrictions are not enforced.
/// - **Browser WASM**: returns an empty iterator — the WHATWG fetch spec
///   blocks JavaScript from reading `Set-Cookie` for any cross-origin or
///   credentialed response.
///
/// This is a humble object: it has no logic of its own beyond defending
/// against non-UTF-8 header values.
fn collect_set_cookies(response: &reqwest::Response) -> Vec<String> {
    let mut out = Vec::new();
    for val in response.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(raw) = std::str::from_utf8(val.as_bytes()) {
            out.push(raw.to_owned());
        }
    }
    out
}
