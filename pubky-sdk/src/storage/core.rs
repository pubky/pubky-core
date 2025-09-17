use pkarr::PublicKey;
use reqwest::{Method, RequestBuilder};
use url::Url;

use super::resource::IntoPubkyResource;
use crate::{
    PubkyHttpClient, PubkyResource, PubkySession,
    errors::{RequestError, Result},
    global::global_client,
};

/// High-level file/HTTP API against a Pubky homeserver.
///
/// `PubkyStorage` operates in two modes:
///
/// ### 1) Session mode (authenticated)
/// Obtained from a session via [`crate::PubkySession::storage`]. In this mode:
/// - Requests are **scoped to that user's session** by default (relative paths resolve to that user).
/// - On native targets, the session cookie is **automatically attached** to requests
///   targeting *that same user’s* homeserver.
/// - Reads **and** writes are expected to succeed (subject to server authorization).
///
/// ```no_run
/// # use pubky::{PubkyAuthRequest, Capabilities};
/// # async fn example() -> pubky::Result<()> {
/// #   let caps = Capabilities::default();
/// #   let (sub, _url) = PubkyAuthRequest::new(&caps)?.subscribe();
/// #   // ... a signer (e.g. Pubky Ring) posts a token for this URL ...
/// #   let session = sub.wait_for_approval().await?;
///     // Relative resource paths are resolved against the user's session.
///     session.storage().put("/pub/app/hello.txt", "hello").await?;
///     let body = session.storage().get("/pub/app/hello.txt").await?.text().await?;
///     assert_eq!(body, "hello");
/// #   Ok(())
/// # }
/// ```
///
/// ### 2) Public mode (unauthenticated)
/// Constructed via [`PubkyStorage::new_public`]. In this mode:
/// - **No session** is attached; requests are unauthenticated.
/// - Resources **must include the target user** (e.g. `"{alice_pubkey}/pub/app/file"`.
///   Relative/session-scoped paths are rejected.
/// - Use for public reads (GET/HEAD/LIST). Writes will be rejected.
///
/// ```no_run
/// # use pubky::PubkyStorage;
/// # async fn example() -> pubky::Result<()> {
///     let storage = PubkyStorage::new_public()?;
///     // Fully-qualified pubky resource: user + path
///     let resp = storage.get("alice_pubky/pub/site/index.html").await?;
///     let html = resp.text().await?;
///     println!("{html}");
/// #   Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PubkyStorage {
    pub(crate) client: PubkyHttpClient,
    /// When `Some(public_key)`, relative paths are session-scoped and cookies may be attached.
    /// When `None`, only absolute user-qualified paths are accepted.
    pub(crate) public_key: Option<PublicKey>,
    pub(crate) has_session: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie: Option<String>,
}

impl PubkyStorage {
    /// Create a **public (unauthenticated)** storage access.
    ///
    /// Use this for read-only access to any user’s public content without a session.
    /// In this mode **resources must be user-qualified** (e.g. `"alice_pubky/pub/..."`).
    ///
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::PubkyStorage;
    /// # async fn example() -> pubky::Result<()> {
    /// let storage = PubkyStorage::new_public()?;
    /// let resp = storage.get("alice/pub/site/index.html").await?;
    /// # Ok(()) }
    /// ```
    pub fn new_public() -> Result<PubkyStorage> {
        Ok(PubkyStorage {
            client: global_client()?,
            public_key: None,
            has_session: false,
            #[cfg(not(target_arch = "wasm32"))]
            cookie: None,
        })
    }

    /// Construct a **session-mode** PubkyStorage from an existing session.
    ///
    /// Equivalent to [`PubkySession::storage()`].
    pub fn new_from_session(session: &PubkySession) -> PubkyStorage {
        PubkyStorage {
            client: session.client.clone(),
            public_key: Some(session.info.public_key().clone()),
            has_session: true,
            #[cfg(not(target_arch = "wasm32"))]
            cookie: Some(session.cookie.clone()),
        }
    }

    /// Resolve a resource into a concrete `pubky://…` or `https://…` URL for this storage.
    ///
    /// - **Session mode:** relative paths are scoped to this storage’s user.
    /// - **Public mode:** the resource must include the target user; relative/session-scoped paths error.
    pub(crate) fn to_url<P: IntoPubkyResource>(&self, p: P) -> Result<Url> {
        let addr: PubkyResource = p.into_pubky_resource()?;

        let url_str = match (&self.public_key, &addr.user) {
            // Session mode: default to this user for session-scoped paths
            (Some(default_user), _) => addr.to_pubky_url(Some(default_user))?,
            // Public mode + explicit user in the input => OK
            (None, Some(_user_in_addr)) => addr.to_pubky_url(None)?,
            // Public mode + session-scoped resource => reject (no default user available)
            (None, None) => {
                return Err(RequestError::Validation {
                    message: "public storage requires user-qualified path: use `<user>/<path>` or `pubky://<user>/<path>`".into(),
                }
                .into())
            }
        };

        Ok(Url::parse(&url_str)?)
    }

    /// Build a request for this storage. If `resource` is relative, it targets this storage’s user (session mode).
    ///
    /// On native targets, the session cookie is attached **only** when the URL points to the
    /// same user bound to this storage (i.e., cookies never leak across users).
    pub(crate) async fn request<P: IntoPubkyResource>(
        &self,
        method: Method,
        resource: P,
    ) -> Result<RequestBuilder> {
        let url = self.to_url(resource)?;
        let rb = self.client.cross_request(method, url.clone()).await?;
        #[cfg(not(target_arch = "wasm32"))]
        let rb = self.maybe_attach_session_cookie(&url, rb);
        Ok(rb)
    }
}

// ---- Cookie attachment (native only) ----
#[cfg(not(target_arch = "wasm32"))]
impl PubkyStorage {
    fn is_this_users_homeserver(&self, url: &Url) -> bool {
        let Some(user) = &self.public_key else {
            return false;
        };
        let host = url.host_str().unwrap_or("");
        if let Some(tail) = host.strip_prefix("_pubky.") {
            PublicKey::try_from(tail).ok().is_some_and(|h| &h == user)
        } else {
            PublicKey::try_from(host).ok().is_some_and(|h| &h == user)
        }
    }

    pub(crate) fn maybe_attach_session_cookie(
        &self,
        url: &Url,
        rb: RequestBuilder,
    ) -> RequestBuilder {
        if !self.has_session {
            return rb;
        }
        if !self.is_this_users_homeserver(url) {
            return rb;
        }
        let Some(user) = &self.public_key else {
            return rb;
        };
        let Some(secret) = self.cookie.as_ref() else {
            return rb;
        };
        let cookie_name = user.to_string();
        rb.header(reqwest::header::COOKIE, format!("{cookie_name}={secret}"))
    }
}
