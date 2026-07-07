//! Cookie credential — legacy auth flow.
//!
//! This is the **legacy** session credential. It will be removed once all
//! ecosystem clients have migrated to the grant flow. Retirement is a folder
//! delete: `rm -rf actors/auth/cookie/` plus dropping the cookie arm in the
//! facade.
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

use std::any::Any;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use pubky_common::{auth::AuthToken, crypto::PublicKey, session::CookieSessionRecord};

use reqwest::{Method, RequestBuilder, Response};

use crate::actors::session::core::PubkySession;
use crate::actors::session::credential::{SessionCredential, credential_session_missing};
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
pub struct CookieCredential {
    /// User public key — used to name the `Cookie` header.
    user: PublicKey,
    /// Full cookie session record for cookie-specific view access.
    record: Arc<RwLock<CookieSessionRecord>>,
    /// Cookie secret captured from `Set-Cookie`. `None` only on browser
    /// WASM where the value is hidden by the fetch spec.
    cookie: Option<String>,
    /// Homeserver this cookie was established against, once known. Gates
    /// [`SessionCredential::can_attach_to`] so the cookie is only ever attached
    /// to a request targeting that homeserver.
    homeserver: Arc<RwLock<Option<PublicKey>>>,
}

impl CookieCredential {
    /// Create a cookie credential from a [`CookieSessionRecord`].
    pub(crate) fn new(
        user: PublicKey,
        cookie: Option<String>,
        record: CookieSessionRecord,
        homeserver: Option<PublicKey>,
    ) -> Self {
        Self {
            user,
            record: Arc::new(RwLock::new(record)),
            cookie,
            homeserver: Arc::new(RwLock::new(homeserver)),
        }
    }

    /// Record the homeserver this cookie is bound to. Idempotent.
    pub(crate) fn set_homeserver(&self, homeserver: PublicKey) {
        if let Ok(mut hs) = self.homeserver.write() {
            *hs = Some(homeserver);
        }
    }

    /// The homeserver this cookie is currently bound to, if any.
    fn bound_homeserver(&self) -> Option<PublicKey> {
        self.homeserver.read().ok().and_then(|hs| hs.clone())
    }

    /// Build a cookie credential from a successful `/session` or `/signup`
    /// response. `homeserver` is the homeserver that served it, when known.
    pub(crate) async fn from_response(
        response: Response,
        homeserver: Option<PublicKey>,
    ) -> Result<Self> {
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
        Ok(Self::new(user, cookie, record, homeserver))
    }

    /// Establish a cookie credential from a signed [`AuthToken`] (legacy flow).
    ///
    /// POSTs the token to the homeserver's `/session` endpoint and constructs
    /// a [`CookieCredential`] ready to be lifted into a [`PubkySession`] via
    /// [`PubkySession::from_cookie_credential`]. `homeserver` is the homeserver
    /// the session is established against, when the caller knows it.
    pub(crate) async fn from_auth_token(
        token: &AuthToken,
        client: &PubkyHttpClient,
        homeserver: Option<PublicKey>,
    ) -> Result<Self> {
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
        Self::from_response(response, homeserver).await
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
// `Send`. See [`crate::actors::session::credential::SessionCredential`] for
// the full rationale.
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

    async fn signout(&self, client: &PubkyHttpClient) -> Result<()> {
        let url = format!("pubky{}/session", self.user.z32());
        let resolved = resolve_pubky(&url)?;
        let rb = client.cross_request(Method::DELETE, resolved).await?;
        let rb = self.attach(rb, client).await?;
        let response = rb.send().await.map_err(crate::Error::from)?;
        check_http_status(response).await?;
        Ok(())
    }

    async fn attach(
        &self,
        rb: RequestBuilder,
        _client: &PubkyHttpClient,
    ) -> Result<RequestBuilder> {
        // When we own the secret (native, Node.js WASM) we attach it manually.
        match &self.cookie {
            Some(cookie) => {
                let cookie_name = self.user.z32();
                Ok(rb.header(reqwest::header::COOKIE, format!("{cookie_name}={cookie}")))
            }
            None => {
                // Browser WASM: the secret lives only in the cookie jar. Opt this
                // request into credentialed fetch so the jar is sent (event streams
                // default to `credentials: omit`). Only reached once `can_attach_to`
                // confirmed the target is this cookie's bound homeserver.
                #[cfg(target_arch = "wasm32")]
                {
                    Ok(rb.fetch_credentials_include())
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Ok(rb)
                }
            }
        }
    }

    async fn can_attach_to(&self, homeserver: &PublicKey) -> bool {
        // Attach only to the homeserver this cookie was established against. An
        // unbound cookie (restored, not yet revalidated) fails closed.
        self.bound_homeserver().as_ref() == Some(homeserver)
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

        // Best-effort hydration for restored/imported cookies that were created
        // without a homeserver binding. Normal signup/signin paths bind at
        // construction. This can theoretically race with concurrent PKDNS
        // re-homing between the request resolution and this lookup; if resolution
        // fails, keep the credential unbound so private event streams stay
        // anonymous.
        if self.bound_homeserver().is_none()
            && let Some(hs) = crate::Pkdns::with_client(client.clone())
                .get_homeserver_of(user)
                .await
        {
            self.set_homeserver(hs);
        }
        Ok(Some(info))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl PubkySession {
    /// Build a cookie-backed [`PubkySession`] from a [`CookieCredential`].
    ///
    /// Typical use: after
    /// [`PubkyCookieAuthFlow::await_credential`](crate::PubkyCookieAuthFlow::await_credential)
    /// returns a credential you want to hold separately, this lifts it into
    /// a full session bound to the given HTTP client.
    #[must_use]
    pub fn from_cookie_credential(client: PubkyHttpClient, credential: CookieCredential) -> Self {
        Self::from_credential(client, Arc::new(credential))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::{
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
    };

    fn cookie_credential(user: &PublicKey, homeserver: Option<PublicKey>) -> CookieCredential {
        let record =
            CookieSessionRecord::new(user, Capabilities::from(vec![Capability::root()]), None);
        CookieCredential::new(
            user.clone(),
            Some("cookie-secret".to_string()),
            record,
            homeserver,
        )
    }

    /// A bound cookie attaches only to the homeserver it was established against.
    #[tokio::test]
    async fn can_attach_to_only_matches_bound_homeserver() {
        let user = Keypair::random().public_key();
        let bound = Keypair::random().public_key();
        let other = Keypair::random().public_key();
        let credential = cookie_credential(&user, Some(bound.clone()));

        assert!(credential.can_attach_to(&bound).await);
        assert!(!credential.can_attach_to(&other).await);
    }

    /// An unbound cookie (restored, not yet revalidated) fails closed, then
    /// attaches to its homeserver once bound.
    #[tokio::test]
    async fn can_attach_to_is_false_until_bound() {
        let user = Keypair::random().public_key();
        let credential = cookie_credential(&user, None);
        let homeserver = Keypair::random().public_key();

        assert!(!credential.can_attach_to(&homeserver).await);

        credential.set_homeserver(homeserver.clone());
        assert!(credential.can_attach_to(&homeserver).await);
        assert!(
            !credential
                .can_attach_to(&Keypair::random().public_key())
                .await
        );
    }
}
