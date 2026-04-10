use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use pubky_common::{crypto::PublicKey, session::SessionInfo};
use reqwest::Method;

use super::credential::{CookieCredential, SessionCredential};
use super::view::{CookieSessionView, JwtSessionView};
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{AuthError, RequestError};
use crate::{
    AuthToken, Error, PubkyHttpClient, Result, SessionStorage, cross_log, util::check_http_status,
};

/// Stateful, per-identity API driver built on a shared [`PubkyHttpClient`].
///
/// A `PubkySession` represents one user/identity authenticated to a homeserver.
/// It hides the choice of credential (legacy cookie vs grant-based JWT) behind
/// a single API: callers always go through `info()`, `storage()`, `revalidate()`,
/// `signout()`, etc., and the SDK dispatches to the right wire format internally
/// via an internal credential trait.
///
/// What it does:
/// - Attaches the correct authentication header (`Cookie` or `Authorization: Bearer`)
///   to requests targeting this user's homeserver.
/// - Exposes homeserver verbs (`get/put/post/patch/delete/head`) scoped to this identity.
/// - Implements identity flows: `signup`, `signin`, `signout`, `session`, and pubkyauth.
///
/// To access JWT-only or cookie-only operations (grant management, secret
/// export), use the capability-view accessors [`Self::as_jwt`] and
/// [`Self::as_cookie`].
///
/// Concurrency:
/// - `PubkySession` is cheap to clone and thread-safe; it shares the underlying
///   [`PubkyHttpClient`] and credential state via `Arc`.
#[derive(Clone)]
pub struct PubkySession {
    pub(crate) client: PubkyHttpClient,
    pub(crate) credential: Arc<dyn SessionCredential>,
}

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
        Ok(Self {
            client,
            credential: Arc::new(credential),
        })
    }

    /// Build a session from a fully-formed credential. Used by the JWT-mode
    /// constructors in [`super::jwt`] and the cookie import paths.
    pub(crate) fn from_credential(
        client: PubkyHttpClient,
        credential: Arc<dyn SessionCredential>,
    ) -> Self {
        Self { client, credential }
    }

    /// Returns a snapshot of the current session info.
    ///
    /// For JWT-backed sessions this is a synthesized [`SessionInfo`] derived
    /// from the most recent `GrantSessionInfo` returned by the homeserver — it
    /// carries the user public key, capabilities, and creation timestamp.
    ///
    /// `SessionInfo` is small and `Clone`-cheap; this method returns by value
    /// so the API is uniform across credential types and reads never block on
    /// JWT refresh.
    ///
    /// # Panics
    /// - Panics if the internal `RwLock` is poisoned. This is only possible if
    ///   another thread panicked while holding the lock — extremely unlikely
    ///   in normal operation since the critical sections are tiny.
    #[must_use]
    pub fn info(&self) -> SessionInfo {
        self.credential
            .info()
            .read()
            .expect("PubkySession::info RwLock poisoned")
            .clone()
    }

    /// Returns a reference to the internal `PubkyHttpClient`.
    ///
    /// Raw transport handle. No per-session credential injection. Use `storage()`
    /// for authenticated, session-scoped requests.
    #[must_use]
    pub const fn client(&self) -> &PubkyHttpClient {
        &self.client
    }

    /// Internal accessor for the credential.
    pub(crate) fn credential(&self) -> &Arc<dyn SessionCredential> {
        &self.credential
    }

    /// User public key for this session (cheap clone of the cached snapshot).
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        self.info().public_key().clone()
    }

    /// Round-trip the current session with the homeserver to verify it's still valid.
    ///
    /// Returns:
    /// - `Ok(Some(session))` if the server recognizes and returns the session (still valid).
    /// - `Ok(None)` if the session no longer exists (expired/invalidated).
    /// - `Err(_)` for transport or server errors unrelated to validity.
    ///
    /// This does *not* mutate the session; it's a sanity/validity check.
    ///
    /// # Errors
    /// - Propagates transport failures from the session endpoint.
    /// - Returns [`crate::errors::Error::Authentication`] if the homeserver rejects the request.
    pub async fn revalidate(&self) -> Result<Option<SessionInfo>> {
        let user = self.info().public_key().clone();
        cross_log!(info, "Revalidating session for {}", user);
        self.credential.revalidate(&self.client, &user).await
    }

    /// Sign out and invalidate this session server-side.
    ///
    /// - **On success:** the session is consumed (dropped).
    /// - **On failure:** you get `(Error, Self)` back so you can retry or inspect.
    ///
    /// # Errors
    /// - Returns the original [`crate::errors::Error`] alongside `self` when the transport
    ///   request fails or the homeserver responds with a non-success status.
    pub async fn signout(self) -> std::result::Result<(), (Error, Self)> {
        cross_log!(info, "Signing out session for {}", self.info().public_key());
        let path = self.credential.signout_path();
        let resp = match self.storage().request(Method::DELETE, path).await {
            Ok(rb) => match rb.send().await {
                Ok(r) => r,
                Err(e) => return Err((Error::from(e), self)),
            },
            Err(e) => return Err((e, self)),
        };
        if let Err(e) = check_http_status(resp).await {
            cross_log!(error, "Signout failed: {}", e);
            return Err((e, self));
        }
        cross_log!(info, "Session signed out");
        Ok(())
    }

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

    /// Returns a [`JwtSessionView`] if this session is JWT-backed.
    ///
    /// JWT-only operations (`list_grants`, `revoke_grant`, `current_jwt`,
    /// `force_refresh`, `grant_id`) live on the view. Cookie-backed sessions
    /// return `None`.
    #[must_use]
    pub fn as_jwt(&self) -> Option<JwtSessionView<'_>> {
        self.credential
            .as_jwt()
            .map(|c| JwtSessionView::new(self, c))
    }

    /// Returns a [`CookieSessionView`] if this session is cookie-backed.
    ///
    /// Cookie-only operations live on the view. JWT-backed sessions return
    /// `None`. The view is available on every target — what differs by
    /// runtime is whether the cookie secret is *capturable*: see
    /// [`CookieSessionView::export_secret`].
    #[must_use]
    pub fn as_cookie(&self) -> Option<CookieSessionView<'_>> {
        self.credential
            .as_cookie()
            .map(|c| CookieSessionView::new(self, c))
    }

    /// Create a **session-mode** Storage bound to this user session.
    ///
    /// - Relative paths (e.g. `"pub/my-cool-app/file"`) are resolved to **this** user.
    /// - Requests that target this user's homeserver automatically carry the
    ///   session cookie or bearer JWT, depending on the credential.
    ///
    /// See [`SessionStorage`] for usage examples.
    #[must_use]
    pub fn storage(&self) -> SessionStorage {
        SessionStorage::new(self)
    }
}

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

impl std::fmt::Debug for PubkySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("PubkySession");
        ds.field("client", &self.client);
        ds.field("credential", &self.credential);
        ds.field("info", &self.info());
        ds.finish()
    }
}
