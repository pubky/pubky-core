//! Client <=> Signer authing (“pubkyauth”) as a single, self-contained flow.
//!
//! ## TL;DR (happy path)
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let flow = PubkyAuthFlow::start(&caps)?; // starts background polling immediately
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! // Blocks until the signer (e.g., Pubky Ring) approves and server issues a session.
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ## Custom relay / non-blocking UI
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow};
//! # use std::time::Duration;
//! # async fn ui() -> pubky::Result<()> {
//! let flow = PubkyAuthFlow::builder(&Capabilities::default())
//!     .relay(url::Url::parse("http://localhost:8080/link/")?) // your relay
//!     .start()?; // starts background polling immediately
//!
//! // show_qr(flow.authorization_url()); // render QR or deeplink in your UI
//!
//! loop {
//!     if let Some(session) = flow.try_poll_once().await? {
//!         // on_logged_in(session);
//!         break;
//!     }
//!     tokio::time::sleep(Duration::from_millis(300)).await;
//! }
//! # Ok(()) }
//! ```
//!
//! ## How it works (security)
//! Each flow generates a random `client_secret` (32 bytes). The relay **channel id**
//! is `base64url( hash(client_secret) )`. The signer encrypts an `AuthToken` with
//! `client_secret` and POSTs it to the channel; your app long-polls `GET` on the same
//! URL and decrypts the payload locally. The relay **cannot decrypt anything**, it
//! simply forwards bytes.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::{AbortHandle, Abortable};

use reqwest::Method;
use url::Url;

use pubky_common::{
    crypto::decrypt,
    crypto::{hash, random_bytes},
};

use crate::{
    AuthToken, Capabilities, PubkyHttpClient, PubkySession,
    errors::{AuthError, Error, Result},
    util::check_http_status,
};

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

/// Default HTTP relay base when none is supplied.
///
/// The per-flow channel segment is appended automatically as:
/// `base + base64url(hash(client_secret))`.
///
/// A trailing slash on `base` is optional; we normalize paths.
pub const DEFAULT_HTTP_RELAY: &str = "https://httprelay.pubky.app/link/";

/// End-to-end **auth flow** (request + live polling) you *hold on to*.
///
/// Use it like this:
/// 1. Construct with [`PubkyAuthFlow::start`] (happy path) or the builder
///    [`PubkyAuthFlow::builder`] to override relay/client.
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to the signer.
/// 3. Complete the flow with [`await_approval`](Self::await_approval) **or**
///    poll with [`try_poll_once`](Self::try_poll_once) / [`try_token`](Self::try_token).
///
/// Background polling **starts immediately** at construction. Dropping this value cancels
/// the background task; the relay channel itself expires server-side after its TTL.
#[derive(Debug, Clone)]
pub struct PubkyAuthFlow {
    client: PubkyHttpClient,
    auth_url: Url,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl PubkyAuthFlow {
    /// Start a flow with the default HTTP relay.
    ///
    /// Spawns the background poller immediately and returns a handle.
    pub fn start(caps: &Capabilities) -> Result<Self> {
        PubkyAuthFlowBuilder::new(caps.clone()).start()
    }

    /// Create a builder to override **relay** and/or provide a custom **client**.
    pub fn builder(caps: &Capabilities) -> PubkyAuthFlowBuilder {
        PubkyAuthFlowBuilder::new(caps.clone())
    }

    /// The `pubkyauth://` deep link you display (QR/URL) to the signer.
    ///
    /// Contains the **capabilities**, **client_secret** (base64url), and **relay** base.
    pub fn authorization_url(&self) -> &Url {
        &self.auth_url
    }

    /// Block until the signer approves and the server issues a session.
    ///
    /// This awaits the background poller’s result, verifies/decrypts the token,
    /// and completes the `/session` exchange to return a ready-to-use [`PubkySession`].
    pub async fn await_approval(self) -> Result<PubkySession> {
        let client = self.client.clone();
        let token = self.recv_token().await?;
        PubkySession::new(&token, client).await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// This awaits the background poller’s result.
    pub async fn await_token(self) -> Result<AuthToken> {
        self.recv_token().await
    }

    async fn recv_token(&self) -> Result<AuthToken> {
        match self.rx.recv_async().await {
            Ok(res) => res,
            Err(_) => Err(AuthError::RequestExpired.into()),
        }
    }

    /// Non-blocking probe (single step) that **consumes any ready token** and returns:
    /// - `Ok(Some(session))` when a token was delivered and the session established.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    pub async fn try_poll_once(&self) -> Result<Option<PubkySession>> {
        if let Some(tok) = self.try_token() {
            let token = tok?;
            return Ok(Some(PubkySession::new(&token, self.client.clone()).await?));
        }
        Ok(None)
    }

    /// Non-blocking check: returns a verified `AuthToken` if the background poller has delivered it.
    ///
    /// - `Some(Ok(AuthToken))` when ready.
    /// - `Some(Err(_))` if the background task failed (expired/transport error).
    /// - `None` if not yet delivered.
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        self.rx.try_recv().ok()
    }

    // -- internals --

    /// Long-poll until a token arrives or the channel expires. Runs in the background task.
    async fn poll_for_token_loop(
        client: PubkyHttpClient,
        relay_channel_url: Url,
        client_secret: [u8; 32],
        tx: flume::Sender<Result<AuthToken>>,
    ) {
        let result = Self::poll_for_token(&client, &relay_channel_url, &client_secret).await;
        let _ = tx.send(result);
    }
}

impl PubkyAuthFlow {
    async fn poll_for_token(
        client: &PubkyHttpClient,
        relay_channel_url: &Url,
        client_secret: &[u8; 32],
    ) -> Result<AuthToken> {
        use reqwest::StatusCode;

        let response = Self::poll_channel(client, relay_channel_url).await?;

        if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
            return Err(AuthError::RequestExpired.into());
        }

        let response = check_http_status(response).await?;
        let encrypted = response.bytes().await?;

        Self::decode_token(&encrypted, client_secret)
    }

    async fn poll_channel(
        client: &PubkyHttpClient,
        relay_channel_url: &Url,
    ) -> Result<reqwest::Response> {
        loop {
            match Self::poll_channel_once(client, relay_channel_url).await {
                Ok(response) => return Ok(response),
                Err(PollError::Timeout) => continue,
                Err(PollError::Failure(err)) => return Err(err),
            }
        }
    }

    async fn poll_channel_once(
        client: &PubkyHttpClient,
        relay_channel_url: &Url,
    ) -> std::result::Result<reqwest::Response, PollError> {
        let request = client
            .cross_request(Method::GET, relay_channel_url.clone())
            .await
            .map_err(PollError::Failure)?;

        match request.send().await {
            Ok(response) => Ok(response),
            Err(err) if err.is_timeout() => Err(PollError::Timeout),
            Err(err) => Err(PollError::Failure(err.into())),
        }
    }

    fn decode_token(encrypted: &[u8], client_secret: &[u8; 32]) -> Result<AuthToken> {
        let token_bytes = decrypt(encrypted, client_secret)?;
        Ok(AuthToken::verify(&token_bytes)?)
    }
}

enum PollError {
    Timeout,
    Failure(Error),
}

impl Drop for PubkyAuthFlow {
    fn drop(&mut self) {
        // Stop background polling immediately.
        self.abort.abort();
    }
}

/// Builder for [`PubkyAuthFlow`].
///
/// Use to override the HTTP relay and/or the `PubkyHttpClient`.
#[derive(Debug, Clone)]
pub struct PubkyAuthFlowBuilder {
    caps: Capabilities,
    relay: Option<Url>,
    client: Option<PubkyHttpClient>,
}

impl PubkyAuthFlowBuilder {
    pub(crate) fn new(caps: Capabilities) -> Self {
        Self {
            caps,
            relay: None,
            client: None,
        }
    }

    /// Set a custom relay base URL. The flow will append the per-channel segment
    /// as `base + base64url(hash(client_secret))`. Trailing slash optional.
    pub fn relay(mut self, relay: Url) -> Self {
        self.relay = Some(relay);
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn the background poller,
    /// and return the flow handle.
    pub fn start(self) -> Result<PubkyAuthFlow> {
        let client = match self.client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        // 1) Resolve relay base (default if not provided).
        let mut relay = match self.relay {
            Some(u) => u,
            None => Url::parse(DEFAULT_HTTP_RELAY)?,
        };

        // 2) Generate client secret and build pubkyauth:// URL (caps + secret + relay).
        let client_secret = random_bytes::<32>();
        let auth_url = Url::parse(&format!(
            "pubkyauth:///?caps={}&secret={}&relay={}",
            self.caps,
            URL_SAFE_NO_PAD.encode(client_secret),
            relay
        ))?;

        // 3) Append derived channel id to the relay URL.
        //    channel_id = base64url( hash(client_secret) )
        {
            let mut segs = relay
                .path_segments_mut()
                .map_err(|_| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
            segs.pop_if_empty(); // normalize trailing slash
            let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
            segs.push(&channel_id);
        }

        // 4) Spawn background polling (single-shot delivery)
        let (tx, rx) = flume::bounded(1);
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let bg_client = client.clone();
        let bg_relay = relay.clone();
        let bg_secret = client_secret;

        let fut = async move {
            PubkyAuthFlow::poll_for_token_loop(bg_client, bg_relay, bg_secret, tx).await;
        };

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(Abortable::new(fut, abort_reg));

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));

        Ok(PubkyAuthFlow {
            client,
            auth_url,
            rx,
            abort: abort_handle,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Capabilities;

    #[tokio::test]
    async fn constructs_urls_and_channel() {
        let caps = Capabilities::default();
        let flow = PubkyAuthFlow::start(&caps).unwrap();

        // pubkyauth:// deep link contains caps + secret + relay
        assert!(
            flow.authorization_url()
                .as_str()
                .starts_with("pubkyauth:///?caps=")
        );
        assert!(
            flow.authorization_url()
                .query_pairs()
                .any(|(k, _)| k == "secret")
        );
        assert!(
            flow.authorization_url()
                .query_pairs()
                .any(|(k, _)| k == "relay")
        );
    }
}
