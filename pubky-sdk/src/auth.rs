use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::{AbortHandle, Abortable};
use reqwest::Method;
use url::Url;

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

use crate::{
    Capabilities, PubkyAgent, PubkyHttpClient, cross_debug,
    errors::{AuthError, Result},
    global::global_client,
    util::check_http_status,
};
use pubky_common::{
    auth::AuthToken,
    crypto::{decrypt, hash, random_bytes},
};

/// Default HTTP relay when none is supplied.
///
/// The per-flow channel segment is appended automatically as:
/// `base + base64url(hash(client_secret))`.
/// A trailing slash on `base` is optional.
pub const DEFAULT_HTTP_RELAY: &str = "https://httprelay.pubky.app/link/";

/// Pubkyauth handshake for keyless apps.
///
/// One `PubkyPairingAuth` <=> one relay channel (single-use).
///
/// Typical usage:
/// 1. Create with [`PubkyPairingAuth::new`].
/// 2. Call [`PubkyPairingAuth::subscribe`] to start background polling and obtain the `pubkyauth://` URL.
/// 3. Show the returned URL (QR/deeplink) to the signing device (e.g., Pubky Ring).
/// 4. Await [`AuthSubscription::wait_for_approval`] to obtain a session-bound [`PubkyAgent`].
///
/// Threading:
/// - `PubkyPairingAuth` is cheap to construct; polling runs in a single abortable task spawned by `subscribe`.
#[derive(Debug)]
pub struct PubkyPairingAuth {
    client: PubkyHttpClient,
    client_secret: [u8; 32],
    pubkyauth_url: Url,
    relay_channel_url: Url,
}

impl PubkyPairingAuth {
    /// Build an auth flow bound to a specific `PubkyHttpClient`.
    ///
    /// # Relay selection
    /// - If `relay` is `Some`, that URL is used as the relay base (trailing slash optional).
    /// - If `relay` is `None`, the flow defaults to [`DEFAULT_HTTP_RELAY`], a Synonym-hosted
    ///   instance. **If that relay is unavailable your login cannot complete.** For larger
    ///   or production apps, prefer running your own relay and passing its base URL here.
    ///
    /// # What is an [HTTP relay](https://httprelay.io)?
    /// A tiny server that provides one-shot “link” channels for **producer => consumer**
    /// delivery: your app long-polls `GET /link/<channel>`, and the signer `POST`s the
    /// encrypted token to the same channel; the relay just forwards bytes (no keys or
    /// Pubky logic required). See the HTTP Relay docs for the `link` method.
    ///
    /// # Self-hosting a relay
    /// HTTP Relay is open-source (MIT). You can run it from static binaries or Docker. A minimal
    /// Docker quickstart is:
    /// ```sh
    /// docker run -p 8080:8080 jonasjasas/httprelay
    /// ```
    /// Then point this API at your instance, e.g. `Some(Url::parse("http://localhost:8080/link/")?)`.
    /// See the project site for [downloads and options](https://httprelay.io/download/)
    ///
    /// # Security & channel derivation
    /// - The per-flow channel path is `base64url(hash(client_secret))`.
    /// - The AuthToken is **encrypted with `client_secret`**; the relay cannot decrypt it
    ///   (it merely forwards the ciphertext).
    ///
    /// # Capabilities
    /// `caps` are embedded into the `pubkyauth://` URL so the signer can review and approve them.
    ///
    /// # Errors
    /// - Returns URL parse errors for an invalid `relay`.
    ///
    /// Internals:
    /// - Generates a random `client_secret` (32 bytes) and a user-displayable `pubkyauth://` deep link.
    /// - Derives the relay channel from `client_secret` and stores both the deep link and the
    ///   fully-qualified channel URL for subsequent polling.
    pub fn new_with_client(
        client: &PubkyHttpClient,
        relay: Option<impl Into<Url>>,
        caps: &Capabilities,
    ) -> Result<Self> {
        // 1) Resolve relay base
        let mut relay_url = match relay {
            Some(r) => r.into(),
            None => Url::parse(DEFAULT_HTTP_RELAY)?,
        };

        // 2) Generate client secret and user-displayable pubkyauth:// URL.
        let client_secret = random_bytes::<32>();
        let pubkyauth_url = Url::parse(&format!(
            "pubkyauth:///?caps={caps}&secret={}&relay={relay_url}",
            URL_SAFE_NO_PAD.encode(client_secret)
        ))?;

        // 3) Derive the relay channel URL from the client secret hash
        let mut segments = relay_url
            .path_segments_mut()
            .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
        segments.pop_if_empty();
        let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
        segments.push(&channel_id);
        drop(segments);

        Ok(Self {
            client: client.clone(),
            client_secret,
            pubkyauth_url,
            relay_channel_url: relay_url,
        })
    }

    /// Construct bound to a default process-wide shared `PubkyHttpClient`.
    /// This is what you want to use for most of your apps.
    ///
    /// # Relay selection
    /// - If `relay` is `Some`, that URL is used as the relay base (trailing slash optional).
    /// - If `relay` is `None`, the flow defaults to [`DEFAULT_HTTP_RELAY`], a Synonym-hosted
    ///   instance. **If that relay is unavailable your login cannot complete.** For larger
    ///   or production apps, prefer running your own relay and passing its base URL here.
    ///
    /// # What is an [HTTP relay](https://httprelay.io)?
    /// A tiny server that provides one-shot “link” channels for **producer => consumer**
    /// delivery: your app long-polls `GET /link/<channel>`, and the signer `POST`s the
    /// encrypted token to the same channel; the relay just forwards bytes (no keys or
    /// Pubky logic required). See the HTTP Relay docs for the `link` method.
    ///
    /// # Self-hosting a relay
    /// HTTP Relay is open-source (MIT). You can run it from static binaries or Docker. A minimal
    /// Docker quickstart is:
    /// ```sh
    /// docker run -p 8080:8080 jonasjasas/httprelay
    /// ```
    /// Then point this API at your instance, e.g. `Some(Url::parse("http://localhost:8080/link/")?)`.
    /// See the project site for [downloads and options](https://httprelay.io/download/)
    ///
    /// # Security & channel derivation
    /// - The per-flow channel path is `base64url(hash(client_secret))`.
    /// - The AuthToken is **encrypted with `client_secret`**; the relay cannot decrypt it
    ///   (it merely forwards the ciphertext).
    ///
    /// # Capabilities
    /// `caps` are embedded into the `pubkyauth://` URL so the signer can review and approve them.
    ///
    /// # Errors
    /// - Returns URL parse errors for an invalid `relay`.
    ///
    /// Internals:
    /// - Generates a random `client_secret` (32 bytes) and a user-displayable `pubkyauth://` deep link.
    /// - Derives the relay channel from `client_secret` and stores both the deep link and the
    ///   fully-qualified channel URL for subsequent polling.
    pub fn new_with_relay(relay: impl Into<Url>, caps: &Capabilities) -> Result<Self> {
        Self::new_with_client(&global_client()?, Some(relay), caps)
    }

    /// Construct bound to a default process-wide shared `PubkyHttpClient`.
    /// This is what you want to use for quick and dirty projects and examples.
    ///
    /// The flow defaults to [`DEFAULT_HTTP_RELAY`], a Synonym-hosted instance.
    /// For larger or production apps, prefer running your own relay and passing
    /// its base URL to [`Self::new_with_relay`]
    ///
    /// # What is an [HTTP relay](https://httprelay.io)?
    /// A tiny server that provides one-shot “link” channels for **producer => consumer**
    /// delivery: your app long-polls `GET /link/<channel>`, and the signer `POST`s the
    /// encrypted token to the same channel; the relay just forwards bytes (no keys or
    /// Pubky logic required). See the HTTP Relay docs for the `link` method.
    ///
    /// # Capabilities
    /// `caps` are embedded into the `pubkyauth://` URL so the signer can review and approve them.
    ///
    /// Internals:
    /// - Generates a random `client_secret` (32 bytes) and a user-displayable `pubkyauth://` deep link.
    /// - Derives the relay channel from `client_secret` and stores both the deep link and the
    ///   fully-qualified channel URL for subsequent polling.
    pub fn new(caps: &Capabilities) -> Result<Self> {
        Self::new_with_client(&global_client()?, None::<Url>, caps)
    }

    /// Return the `pubkyauth://` deep link to display (QR/deeplink) to the signer.
    ///
    /// ⚠️ **Ordering matters if you use [`wait_for_approval`](Self::wait_for_approval).**
    /// `wait_for_approval()` starts polling only when it is called. If you plan to use it,
    /// call `pubkyauth_url()` to display the link and then **immediately** await
    /// `wait_for_approval().await` so approvals aren’t missed during the gap.
    ///
    /// If you want “can’t-miss” semantics without thinking about ordering, use
    /// [`subscribe`](Self::subscribe), which starts polling first and returns both the
    /// subscription handle and this URL.
    pub fn pubkyauth_url(&self) -> &Url {
        &self.pubkyauth_url
    }

    /// Consume the PubkyPairingAuth, start background polling, and return `(subscription, pubkyauth_url)`.
    ///
    /// Semantics:
    /// - Single-shot: delivers at most one token.
    /// - Abortable: dropping the subscription cancels polling immediately; pending `wait_for_token()`/`wait_for_approval()` resolve with `AuthError::RequestExpired`.
    /// - Transport: timeouts are retried in a simple loop; other transport errors propagate.
    ///
    /// Example:
    /// ```
    /// # use pubky::{PubkyPairingAuth, Capabilities};
    /// # async fn test() -> pubky::Result<()> {
    /// let (sub, url) = PubkyPairingAuth::new(&Capabilities::default())?.subscribe();
    /// // Display `url` as QR or deeplink to the Signer ...
    /// # let signer = pubky::PubkySigner::random()?;
    /// # signer.approve_pubkyauth_request(&url).await?;
    /// let agent = sub.wait_for_approval().await?;
    /// # Ok::<(), pubky::Error>(())}
    /// ```
    pub fn subscribe(self) -> (AuthSubscription, Url) {
        let (tx, rx) = flume::bounded(1);

        let client = self.client.clone();
        let relay_channel_url = self.relay_channel_url.clone();
        let client_secret = self.client_secret;

        let (abort_handle, abort_reg) = AbortHandle::new_pair();

        // Background polling future (single-shot delivery)
        let fut = async move {
            let res = Self::poll_for_token(&client, relay_channel_url, client_secret).await;
            // Ignore send failure if the receiver was dropped.
            let _ = tx.send(res);
        };

        // Spawn abortable task cross-platform
        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(Abortable::new(fut, abort_reg));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));

        let auth_subscription = AuthSubscription {
            client: self.client,
            rx,
            abort: abort_handle,
        };
        (auth_subscription, self.pubkyauth_url)
    }

    /// Block (via async/await) until the signer approves, then return a session-bound [`PubkyAgent`].
    ///
    /// This is the ergonomic, single-call variant of [`Self::subscribe`] + [`AuthSubscription::wait_for_approval`]
    /// intended for scripts/CLIs or quickstarts that don’t need to juggle a background handle.
    ///
    /// **How to use**
    /// 1. Build a [`PubkyPairingAuth`], read [`pubkyauth_url`](Self::pubkyauth_url), and display it (QR/deeplink).
    /// 2. Call `wait_for_approval()`. Internally we start a lightweight polling task and await its result.
    /// 3. On success you get a ready-to-use [`PubkyAgent`] with a valid server session.
    ///
    ///
    /// ⚠️ **Important:** `wait_for_approval()` starts polling **when you call it**. If you use
    /// [`pubkyauth_url`](Self::pubkyauth_url) to display the link, you **must** call
    /// `wait_for_approval().await` **immediately after** displaying it. Any delay (e.g., extra I/O, sleeps,
    /// user prompts) can allow a signer to approve before polling begins, increasing the chance of
    /// missing the approval. If you cannot guarantee back-to-back calls, prefer
    /// [`Self::subscribe`], which starts polling before you show the URL.
    ///
    /// **When to prefer this**
    /// - One-shot flows where blocking the current task is fine.
    /// - For non-blocking UIs or multiple concurrent auth flows, use [`Self::subscribe`] and hold the
    ///   returned [`AuthSubscription`] instead.
    ///
    /// **Errors**
    /// - [`AuthError::RequestExpired`]: the relay channel expired/cancelled before a token arrived.
    /// - Other variants of [`crate::Error`] for transport/server/auth failures during sign-in.
    ///
    ///
    /// # Examples
    /// Basic script:
    /// ```no_run
    /// # use pubky::{PubkyPairingAuth, Capabilities};
    /// # async fn run() -> pubky::Result<()> {
    /// let caps = Capabilities::builder().read("/pub/app/").finish();
    /// let auth = PubkyPairingAuth::new(&caps)?;
    /// println!("Scan to sign in: {}", auth.pubkyauth_url());
    /// let agent = auth.wait_for_approval().await?; // must be awaited right when displaying the pubky_auth!
    /// println!("Signed in as {}", agent.public_key());
    /// # Ok(()) }
    /// ```
    pub async fn wait_for_approval(self) -> Result<PubkyAgent> {
        let (sub, _) = self.subscribe();
        sub.wait_for_approval().await
    }

    /// Poll the relay once a background task is running; decrypt and verify the token.
    ///
    /// Status mapping:
    /// - `404`/`410` => [`AuthError::RequestExpired`].
    /// - Non-2xx => mapped via [`check_http_status`].
    /// - Transport timeout => retry loop; other transport errors propagate.
    async fn poll_for_token(
        client: &PubkyHttpClient,
        relay_channel_url: Url,
        client_secret: [u8; 32],
    ) -> Result<AuthToken> {
        use reqwest::StatusCode;

        let response = loop {
            match client
                .cross_request(Method::GET, relay_channel_url.clone())
                .await?
                .send()
                .await
            {
                Ok(r) => break r,
                Err(e) if e.is_timeout() => {
                    cross_debug!("HttpRelay timed out; retrying channel poll …");
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        };

        if response.status() == StatusCode::NOT_FOUND || response.status() == StatusCode::GONE {
            return Err(AuthError::RequestExpired.into());
        }

        let response = check_http_status(response).await?;
        let encrypted = response.bytes().await?;
        let token_bytes = decrypt(&encrypted, &client_secret)?;
        let token = AuthToken::verify(&token_bytes)?;
        Ok(token)
    }
}

/// Handle returned by [`PubkyPairingAuth::subscribe`].
///
/// Owns the background poll task; delivers exactly one `AuthToken` (or an error).
#[derive(Debug)]
#[must_use = "hold on to this and call token().await or wait_for_approval().await to complete the auth flow"]
pub struct AuthSubscription {
    client: PubkyHttpClient,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl AuthSubscription {
    /// Await the verified `AuthToken`.
    ///
    /// Returns:
    /// - `Ok(AuthToken)` on success.
    /// - `Err(AuthError::RequestExpired)` if the relay expired or the subscription was dropped.
    /// - Transport/server errors as appropriate.
    pub async fn wait_for_token(self) -> Result<AuthToken> {
        match self.rx.recv_async().await {
            Ok(res) => res,
            Err(_) => Err(AuthError::RequestExpired.into()),
        }
    }

    /// Await the token and sign in to obtain a session-bound [`PubkyAgent`].
    ///
    /// Steps it does internally:
    /// - Blocks and wait for `AuthToken` via [`AuthSubscription::wait_for_token`].
    /// - POST `pubky://<user>/session` with the token; capture cookie (native) and set pubky.
    /// - Returns the session-bounded [`PubkyAgent`] ready to use.
    ///
    /// Example:
    /// ```
    /// # use pubky::{PubkyPairingAuth, Capabilities};
    /// # async fn test() -> pubky::Result<()> {
    /// let (sub, url) = PubkyPairingAuth::new(&Capabilities::default())?.subscribe();
    /// // display `url` to signer ...
    /// # let signer = pubky::PubkySigner::random()?;
    /// # signer.approve_pubkyauth_request(&url).await?;
    /// let agent = sub.wait_for_approval().await?;
    /// # Ok::<(), pubky::Error>(())}
    /// ```
    pub async fn wait_for_approval(self) -> Result<PubkyAgent> {
        PubkyAgent::new(&self.client.clone(), &self.wait_for_token().await?).await
    }

    /// Non-blocking probe for readiness.
    ///
    /// Returns:
    /// - `Some(Ok(AuthToken))` if already received.
    /// - `Some(Err(_))` if polling failed.
    /// - `None` if not ready yet.
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        self.rx.try_recv().ok()
    }
}

impl Drop for AuthSubscription {
    fn drop(&mut self) {
        // Stop background polling immediately.
        self.abort.abort();
    }
}

#[cfg(test)]
impl PubkyPairingAuth {
    /// Returns the derived relay channel URL, mainly for diagnostics.
    pub fn relay_channel_url(&self) -> &Url {
        &self.relay_channel_url
    }

    /// Decode utilities for testing.
    #[inline]
    pub fn client_secret(&self) -> &[u8; 32] {
        &self.client_secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructs_urls_and_channel() {
        let caps = Capabilities::default();

        let auth = PubkyPairingAuth::new(&caps).unwrap();
        assert!(
            auth.pubkyauth_url()
                .as_str()
                .starts_with("pubkyauth:///?caps=")
        );
        assert!(
            auth.pubkyauth_url()
                .query_pairs()
                .any(|(k, _)| k == "secret")
        );
        assert!(
            auth.relay_channel_url()
                .as_str()
                .starts_with(DEFAULT_HTTP_RELAY)
        );
        // Channel id must be last segment derived from client_secret hash
        let last_seg = auth
            .relay_channel_url()
            .path_segments()
            .and_then(|mut it| it.next_back())
            .unwrap();
        assert_eq!(
            last_seg,
            URL_SAFE_NO_PAD.encode(hash(auth.client_secret()).as_bytes())
        );
    }
}
