use pkarr::PublicKey;
use reqwest::{Method, RequestBuilder};
use url::Url;

use super::path::IntoPubkyPath;
use crate::{
    PubkyHttpClient, PubkyPath,
    errors::{RequestError, Result},
    global::global_client,
};

/// High-level file/HTTP API against a Pubky homeserver.
///
/// `PubkyDrive` operates in two modes:
///
/// ### 1) Session mode (authenticated)
/// Obtained from a session-bound agent via [`crate::PubkyAgent::drive`]. In this mode:
/// - Requests are **scoped to that agent’s user** by default (relative paths resolve to that user).
/// - On native targets, the agent’s session cookie is **automatically attached** to requests
///   targeting *that same user’s* homeserver.
/// - Reads **and** writes are expected to succeed (subject to server authorization).
///
/// ```no_run
/// # use pubky::{PubkyPairingAuth, Capabilities};
/// # async fn example() -> pubky::Result<()> {
/// #   let caps = Capabilities::default();
/// #   let (sub, url) = PubkyPairingAuth::new(None, &caps)?.subscribe();
/// #   // ... a signer (e.g. Pubky Ring) posts a token for this URL ...
/// #   let user = sub.into_agent().await?;
///
///     // Relative paths are resolved against the agent’s user.
///     user.drive().put("/pub/app/hello.txt", "hello").await?;
///     let body = user.drive().get("/pub/app/hello.txt").await?.bytes().await?;
///     assert_eq!(&body, b"hello");
/// #   Ok(())
/// # }
/// ```
///
/// ### 2) Public mode (unauthenticated)
/// Constructed via [`PubkyDrive::public`] or [`PubkyDrive::public_with_client`]. In this mode:
/// - **No session** is attached; requests are unauthenticated.
/// - Paths **must include the target user** (e.g. `"{alice_pubkey}/pub/app/file"`.
///   Relative/agent-scoped paths are rejected.
/// - Use for public reads (GET/HEAD/LIST). Writes will be rejected.
///
/// ```no_run
/// # use pubky::PubkyDrive;
/// # async fn example() -> pubky::Result<()> {
///     let drive = PubkyDrive::public()?;
///     // Fully-qualified path: user + path
///     let resp = drive.get("alice_pubky/pub/site/index.html").await?;
///     let html = resp.text().await?;
///     println!("{html}");
/// #   Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct PubkyDrive {
    pub(crate) client: PubkyHttpClient,
    /// When `Some(public_key)`, relative paths are agent-scoped and cookies may be attached.
    /// When `None`, only absolute user-qualified paths are accepted.
    pub(crate) public_key: Option<PublicKey>,
    pub(crate) has_session: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cookie: Option<String>,
}

impl PubkyDrive {
    /// Create a **public (unauthenticated)** drive that uses the global shared [`PubkyHttpClient`].
    ///
    /// Use this for read-only access to any user’s public content without a session.
    /// In this mode **paths must be user-qualified** (e.g. `"alice_pubky/pub/..."`).
    ///
    /// See also: [`PubkyDrive::public_with_client`].
    ///
    /// # Examples
    /// ```no_run
    /// # use pubky::PubkyDrive;
    /// # async fn example() -> pubky::Result<()> {
    /// let drive = PubkyDrive::public()?;
    /// let resp = drive.get("alice/pub/site/index.html").await?;
    /// # Ok(()) }
    /// ```
    pub fn public() -> Result<PubkyDrive> {
        let client = global_client()?;
        Ok(Self::public_with_client(&client))
    }

    /// Create a **public (unauthenticated)** drive with an explicit client.
    ///
    /// Choose this when you manage your own [`PubkyHttpClient`] (e.g., for connection pooling,
    /// custom TLS/root store, or test wiring).
    ///
    /// In this mode **paths must be user-qualified** (e.g. `"alice/pub/..."`).
    ///
    /// # Examples
    /// ```no_run
    /// # use std::sync::Arc;
    /// # use pubky::{PubkyHttpClient, PubkyDrive};
    /// # async fn example() -> pubky::Result<()> {
    /// let client = PubkyHttpClient::new()?;
    /// let drive = PubkyDrive::public_with_client(Arc::new(client));
    /// let urls = drive.list("alice_pubky/pub/site/").limit(10).send().await?;
    /// # Ok(()) }
    /// ```
    pub fn public_with_client(client: &PubkyHttpClient) -> PubkyDrive {
        PubkyDrive {
            client: client.clone(),
            public_key: None,
            has_session: false,
            #[cfg(not(target_arch = "wasm32"))]
            cookie: None,
        }
    }

    /// Resolve a path into a concrete `pubky://…` or `https://…` URL for this drive.
    ///
    /// - **Session mode:** relative paths are scoped to this drive’s user.
    /// - **Public mode:** the path must include the target user; relative/agent-scoped paths error.
    pub(crate) fn to_url<P: IntoPubkyPath>(&self, p: P) -> Result<Url> {
        let addr: PubkyPath = p.into_pubky_path()?;

        let url_str = match (&self.public_key, &addr.user) {
            // Session mode: default to this agent for agent-scoped paths
            (Some(default_user), _) => addr.to_pubky_url(Some(default_user))?,
            // Public mode + explicit user in the input => OK
            (None, Some(_user_in_addr)) => addr.to_pubky_url(None)?,
            // Public mode + agent-scoped path => reject (no default user available)
            (None, None) => {
                return Err(RequestError::Validation {
                    message: "public drive requires user-qualified path: use `<user>/<path>` or `pubky://<user>/<path>`".into(),
                }
                .into())
            }
        };

        Ok(Url::parse(&url_str)?)
    }

    /// Build a request for this drive. If `path` is relative, it targets this drive’s user (session mode).
    ///
    /// On native targets, the session cookie is attached **only** when the URL points to the
    /// same user bound to this drive (i.e., cookies never leak across users).
    pub(crate) async fn request<P: IntoPubkyPath>(
        &self,
        method: Method,
        path: P,
    ) -> Result<RequestBuilder> {
        let url = self.to_url(path)?;
        let rb = self.client.cross_request(method, url.clone()).await?;
        #[cfg(not(target_arch = "wasm32"))]
        let rb = self.maybe_attach_session_cookie(&url, rb);
        Ok(rb)
    }
}

// ---- Cookie attachment (native only) ----
#[cfg(not(target_arch = "wasm32"))]
impl PubkyDrive {
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
