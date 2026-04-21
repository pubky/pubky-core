//! Cookie credential — legacy auth flow.
//!
//! This is the **legacy** session credential. It will be removed once all
//! ecosystem clients have migrated to the JWT flow. Retirement is a folder
//! delete: `rm -rf credentials/cookie/` plus dropping the cookie arm in
//! [`crate::actors::session::bootstrap`] and the `as_cookie` re-export in
//! [`super::super::super::mod@crate::actors::session`].
//!
//! ## Cross-target behavior
//!
//! The struct shape is identical on every target — only the *availability*
//! of the cookie secret differs by runtime:
//!
//! | Runtime | Set-Cookie visibility | Cookie secret stored | Attach strategy |
//! |---|---|---|---|
//! | Native (`reqwest`) | Yes | Always `Some` | Manual `Cookie` header |
//! | Node.js WASM (undici) | Yes | `Some` | Manual `Cookie` header |
//! | Browser WASM (fetch) | **Hidden by spec** | `None` | Browser cookie jar |
//!
//! The browser case is the only one where we cannot capture the secret —
//! the WHATWG fetch spec hides `Set-Cookie` from JavaScript so the runtime
//! cookie jar is the only place the value lives. On every other runtime
//! the SDK owns the secret and exports/imports just like on native.

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use pubky_common::{auth::AuthToken, crypto::PublicKey, session::CookieSessionRecord};

use reqwest::{Method, RequestBuilder, Response};

use super::super::{SessionCredential, credential_session_missing};
use crate::{
    PubkyHttpClient, actors::session::SessionInfo, actors::storage::resource::resolve_pubky,
    cross_log, errors::Result, util::check_http_status,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::errors::AuthError;

/// Cookie-based session credential (legacy).
///
/// On native and Node.js WASM the credential owns the opaque secret string
/// and replays it on every request. On browser WASM the secret is
/// inaccessible (the fetch spec hides `Set-Cookie`) and the browser cookie
/// jar handles attachment automatically — `cookie` is `None`.
#[derive(Clone, Debug)]
pub(crate) struct CookieCredential {
    /// User public key — used to name the `Cookie` header.
    user: PublicKey,
    /// Full cookie session record for cookie-specific view access.
    record: Arc<RwLock<CookieSessionRecord>>,
    /// Cookie secret captured from `Set-Cookie`. `None` only on browser
    /// WASM where the value is hidden by the fetch spec.
    cookie: Option<String>,
}

impl CookieCredential {
    /// Create a cookie credential from a [`CookieSessionRecord`].
    pub(crate) fn new(
        user: PublicKey,
        cookie: Option<String>,
        record: CookieSessionRecord,
    ) -> Self {
        Self {
            user,
            record: Arc::new(RwLock::new(record)),
            cookie,
        }
    }

    /// Build a cookie credential from a successful `/session` or `/signup`
    /// response.
    pub(crate) async fn from_response(response: Response) -> Result<Self> {
        let raw_set_cookies = collect_set_cookies(&response);

        let bytes = response.bytes().await?;
        let record = CookieSessionRecord::deserialize(&bytes)?;
        let user = record.public_key().clone();
        let cookie_name = user.z32();
        let cookie = raw_set_cookies
            .iter()
            .filter_map(|raw| cookie::Cookie::parse(raw.clone()).ok())
            .find(|c| c.name() == cookie_name)
            .map(|c| c.value().to_string());

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
                "Hydrating WASM cookie credential without captured secret \
                 (browser jar will handle attachment) for {}",
                user
            );
        }

        cross_log!(info, "Hydrated cookie credential for {}", user);
        Ok(Self::new(user, cookie, record))
    }


    /// Establish a session from a signed [`AuthToken`] (legacy cookie flow).
    ///
    /// POSTs the token to the homeserver's `/session` endpoint and constructs a
    /// cookie-based [`PubkySession`].
    pub(crate) async fn from_auth_token(
        token: &AuthToken,
        client: &PubkyHttpClient,
    ) -> Result<Arc<dyn SessionCredential>> {
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
            "Session exchange for {} succeeded; constructing credential",
            token.public_key()
        );
        let credential = Self::from_response(response).await?;
        Ok(Arc::new(credential))
    }

    /// Cookie secret accessor — used by [`super::view::CookieSessionView`]
    /// to export sessions for later restore. Returns `None` on browser WASM.
    pub(crate) fn cookie_secret(&self) -> Option<&str> {
        self.cookie.as_deref()
    }

    /// Returns a clone of the stored [`CookieSessionRecord`].
    pub(crate) fn cookie_record(&self) -> CookieSessionRecord {
        self.record
            .read()
            .expect("CookieCredential record RwLock poisoned")
            .clone()
    }

    /// Replace the cached record — used during revalidation.
    pub(crate) fn replace_record(&self, record: CookieSessionRecord) {
        if let Ok(mut r) = self.record.write() {
            *r = record;
        }
    }
}

/// Cross-target reader for `Set-Cookie` response header values.
fn collect_set_cookies(response: &Response) -> Vec<String> {
    let mut out = Vec::new();
    for val in response.headers().get_all(reqwest::header::SET_COOKIE) {
        if let Ok(raw) = std::str::from_utf8(val.as_bytes()) {
            out.push(raw.to_owned());
        }
    }
    out
}

// Mirrors the cfg pair on the trait definition: native gets `Send` bounds
// for tokio, WASM uses `?Send` because `wasm-bindgen-futures` are not
// `Send`. See `super::super::credential::SessionCredential` for the full
// rationale.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SessionCredential for CookieCredential {
    fn info(&self) -> SessionInfo {
        let record = self
            .record
            .read()
            .expect("CookieCredential record RwLock poisoned");
        SessionInfo::new(record.public_key().clone(), record.capabilities().to_vec())
    }

    fn signout_path(&self) -> &'static str {
        "/session"
    }

    async fn attach(
        &self,
        rb: RequestBuilder,
        _client: &PubkyHttpClient,
    ) -> Result<RequestBuilder> {
        // When we own the secret (native, Node.js WASM) we attach it
        // manually. When we don't (browser WASM) the runtime cookie jar
        // is the source of truth and we leave the request alone.
        match &self.cookie {
            Some(cookie) => {
                let cookie_name = self.user.z32();
                Ok(rb.header(reqwest::header::COOKIE, format!("{cookie_name}={cookie}")))
            }
            None => Ok(rb),
        }
    }

    async fn revalidate(
        &self,
        client: &PubkyHttpClient,
        user: &PublicKey,
    ) -> Result<Option<SessionInfo>> {
        let url = format!("pubky{}/session", user.z32());
        let resolved = resolve_pubky(&url)?;
        let rb = client.cross_request(Method::GET, resolved).await?;
        let rb = self.attach(rb, client).await?;
        let response = rb.send().await.map_err(crate::Error::from)?;
        if credential_session_missing(&response) {
            cross_log!(info, "Cookie session missing on revalidate");
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let bytes = response.bytes().await?;
        let record = CookieSessionRecord::deserialize(&bytes)?;
        let info = SessionInfo::new(record.public_key().clone(), record.capabilities().to_vec());
        self.replace_record(record);
        Ok(Some(info))
    }

    fn as_cookie(&self) -> Option<&CookieCredential> {
        Some(self)
    }
}
