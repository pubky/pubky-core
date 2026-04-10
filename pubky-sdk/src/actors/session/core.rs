use std::sync::Arc;

use pubky_common::crypto::PublicKey;

use super::SessionInfo;
use reqwest::Method;

use super::credential::SessionCredential;
use super::view::{CookieSessionView, JwtSessionView};
use crate::errors::Error;
use crate::{PubkyHttpClient, Result, SessionStorage, cross_log, util::check_http_status};

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
/// Credential-specific factory functions live in dedicated modules:
/// - Cookie: [`super::cookie`] — `credential_from_auth_token`,
///   `session_from_auth_token`, `session_from_cookie_response`, `import_session`,
///   `import_session_secret`, `session_from_secret_file`
/// - JWT: [`super::jwt`] — `credential_from_grant_exchange`,
///   `credential_from_grant_signup`
///
/// Thin delegations on `PubkySession` (`export`, `import`, `import_secret`,
/// `from_secret_file`) preserve the public API surface.
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
    /// Build a session from a fully-formed credential. Used by the JWT-mode
    /// constructors in [`super::jwt`] and the cookie constructors in
    /// [`super::cookie`].
    pub(crate) fn from_credential(
        client: PubkyHttpClient,
        credential: Arc<dyn SessionCredential>,
    ) -> Self {
        Self { client, credential }
    }

    /// Returns the current session info.
    ///
    /// `SessionInfo` is small and `Clone`-cheap; this method returns by value
    /// so the API is uniform across credential types.
    #[must_use]
    pub fn info(&self) -> SessionInfo {
        self.credential.info()
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

impl std::fmt::Debug for PubkySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut ds = f.debug_struct("PubkySession");
        ds.field("client", &self.client);
        ds.field("credential", &self.credential);
        ds.field("info", &self.info());
        ds.finish()
    }
}
