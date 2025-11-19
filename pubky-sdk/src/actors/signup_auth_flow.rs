//! Client <=> Signer signup authing (“pubkysignupauth”) as a single, self-contained flow.
//!
//! ## TL;DR (happy path)
//! ```no_run
//! # use pubky::{Capabilities, PubkySignupAuthFlow};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let invite_code = "1234567890";
//! let homeserver_public_key: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let flow = PubkySignupAuthFlow::builder(&caps, homeserver_public_key).invite_code(invite_code.to_string()).start()?;  // starts background polling immediately
//! println!("Scan to sign up: {}", flow.authorization_url());
//!
//! // Blocks until the signer (e.g., Pubky Ring) approves and server issues a session.
//! let session = flow.await_approval().await?;
//! println!("Signed up as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ## Custom relay / non-blocking UI
//! ```no_run
//! # use pubky::{Capabilities, PubkySignupAuthFlow};
//! # use std::time::Duration;
//! # async fn ui() -> pubky::Result<()> {
//! let homeserver_public_key: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let flow = PubkySignupAuthFlow::builder(&Capabilities::default(), homeserver_public_key)
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

use pkarr::PublicKey;
use reqwest::Method;
use url::Url;

use pubky_common::{
    crypto::decrypt,
    crypto::{hash, random_bytes},
};

use crate::{
    AuthToken, Capabilities, PubkyHttpClient, PubkySession,
    actors::DEFAULT_HTTP_RELAY,
    cross_log,
    errors::{AuthError, Error, Result},
    util::check_http_status,
};

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

/// End-to-end **auth signup flow** (request + live polling) you *hold on to*.
///
/// Use it like this:
/// 1. Construct with [`PubkySignupAuthFlow::start`] (happy path) or the builder
///    [`PubkySignupAuthFlow::builder`] to override relay/client and pass the homeserver public key and optionally the invite code.
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to the signer.
/// 3. Complete the flow with [`await_approval`](Self::await_approval) **or**
///    poll with [`try_poll_once`](Self::try_poll_once) / [`try_token`](Self::try_token).
///
/// Background polling **starts immediately** at construction. Dropping this value cancels
/// the background task; the relay channel itself expires server-side after its TTL.
#[derive(Debug)]
pub struct PubkySignupAuthFlow {
    client: PubkyHttpClient,
    auth_url: Url,
    rx: flume::Receiver<Result<AuthToken>>,
    abort: AbortHandle,
}

impl PubkySignupAuthFlow {
    /// Start a flow with the default HTTP relay.
    ///
    /// Spawns the background poller immediately and returns a handle.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing [`PubkyHttpClient`]
    ///   or generating the relay URL fails.
    pub fn start(caps: &Capabilities, homeserver_public_key: PublicKey) -> Result<Self> {
        PubkySignupAuthFlowBuilder::new(caps.clone(), homeserver_public_key).start()
    }

    /// Create a builder to override **relay** and/or provide a custom **client**.
    #[must_use]
    pub fn builder(
        caps: &Capabilities,
        homeserver_public_key: PublicKey,
    ) -> PubkySignupAuthFlowBuilder {
        PubkySignupAuthFlowBuilder::new(caps.clone(), homeserver_public_key)
    }

    /// The `pubkyauth://` deep link you display (QR/URL) to the signer.
    ///
    /// Contains the **capabilities**, **`client_secret`** (base64url), and **relay** base.
    #[must_use]
    pub const fn authorization_url(&self) -> &Url {
        &self.auth_url
    }

    /// Block until the signer approves and the server issues a session.
    ///
    /// This awaits the background poller’s result, verifies/decrypts the token,
    /// and completes the `/session` exchange to return a ready-to-use [`PubkySession`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay.
    /// - Propagates errors from the internal session exchange if it fails.
    pub async fn await_approval(self) -> Result<PubkySession> {
        let client = self.client.clone();
        let token = self.recv_token().await?;
        PubkySession::new(&token, client).await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// This awaits the background poller’s result.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures encountered while polling the relay.
    pub async fn await_token(self) -> Result<AuthToken> {
        self.recv_token().await
    }

    async fn recv_token(&self) -> Result<AuthToken> {
        self.rx
            .recv_async()
            .await
            .map_err(|_err| AuthError::RequestExpired)?
    }

    /// Non-blocking probe (single step) that **consumes any ready token** and returns:
    /// - `Ok(Some(session))` when a token was delivered and the session established.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expired before a token arrived.
    /// - Propagates HTTP/transport failures from constructing the session.
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
    #[must_use]
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
        cross_log!(
            info,
            "Starting auth signup flow polling for relay channel {}",
            relay_channel_url
        );
        let result = Self::poll_for_token(&client, &relay_channel_url, &client_secret).await;

        if result.is_ok() {
            cross_log!(
                info,
                "Auth signup flow successfully decrypted token for relay channel {}",
                relay_channel_url
            );
        }

        let _ = tx.send(result);
    }

    async fn poll_for_token(
        client: &PubkyHttpClient,
        relay_channel_url: &Url,
        client_secret: &[u8; 32],
    ) -> Result<AuthToken> {
        use reqwest::StatusCode;

        let response = Self::poll_channel(client, relay_channel_url).await?;

        if matches!(response.status(), StatusCode::NOT_FOUND | StatusCode::GONE) {
            cross_log!(
                warn,
                "Auth signup flow relay channel {} expired (status: {})",
                relay_channel_url,
                response.status()
            );
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
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            cross_log!(
                debug,
                "Auth signup flow polling attempt {attempt} requesting {}",
                relay_channel_url
            );

            let result = Self::poll_channel_once(client, relay_channel_url).await;
            if let Some(response) = Self::interpret_poll_result(attempt, relay_channel_url, result)?
            {
                return Ok(response);
            }
        }
    }

    fn interpret_poll_result(
        attempt: u32,
        relay_channel_url: &Url,
        result: std::result::Result<reqwest::Response, PollError>,
    ) -> Result<Option<reqwest::Response>> {
        match result {
            Ok(response) => {
                cross_log!(
                    debug,
                    "Received response for auth signup flow polling attempt {attempt}: status {}",
                    response.status()
                );
                Ok(Some(response))
            }
            Err(PollError::Timeout) => {
                cross_log!(
                    debug,
                    "Auth signup flow polling attempt {attempt} timed out; retrying"
                );
                Ok(None)
            }
            Err(PollError::Failure(err)) => {
                cross_log!(
                    error,
                    "Auth signup flow polling attempt {attempt} failed at {}: {err}",
                    relay_channel_url
                );
                Err(err)
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

impl Drop for PubkySignupAuthFlow {
    fn drop(&mut self) {
        // Stop background polling immediately.
        self.abort.abort();
    }
}

/// Builder for [`PubkySignupAuthFlow`].
///
/// Use to override the HTTP relay, the `PubkyHttpClient`, or set the invite code.
#[derive(Debug, Clone)]
pub struct PubkySignupAuthFlowBuilder {
    // caps: Capabilities,
    // relay: Option<Url>,
    client: Option<PubkyHttpClient>,
    // homeserver_public_key: PublicKey,
    // invite_code: Option<String>,
    url: SignupAuthUrl,
}

impl PubkySignupAuthFlowBuilder {
    pub(crate) fn new(caps: Capabilities, homeserver_public_key: PublicKey) -> Self {
        Self {
            url: SignupAuthUrl::new(homeserver_public_key, caps),
            client: None,
        }
    }

    /// Set a custom relay base URL. The flow will append the per-channel segment
    /// as `base + base64url(hash(client_secret))`. Trailing slash optional.
    pub fn relay(mut self, relay: Url) -> Self {
        self.url.set_relay(relay);
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the invite code to use for the signup flow.
    pub fn invite_code(mut self, invite_code: String) -> Self {
        self.url.set_invite_code(Some(invite_code));
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn the background poller,
    /// and return the flow handle.
    pub fn start(self) -> Result<PubkySignupAuthFlow> {
        let client = match self.client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        let mut relay = self.url.relay().clone();
        let client_secret = *self.url.client_secret();

        // 3) Append derived channel id to the relay URL.
        //    channel_id = base64url( hash(client_secret) )
        {
            let mut segs = relay
                .path_segments_mut()
                .map_err(|()| url::ParseError::RelativeUrlWithCannotBeABaseBase)?;
            segs.pop_if_empty(); // normalize trailing slash
            let channel_id = URL_SAFE_NO_PAD.encode(hash(&client_secret).as_bytes());
            segs.push(&channel_id)
        };

        cross_log!(info, "Auth signup flow derived relay channel {}", relay);

        // 4) Spawn background polling (single-shot delivery)
        let (tx, rx) = flume::bounded(1);
        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        let bg_client = client.clone();
        let bg_relay = relay.clone();
        let bg_secret = client_secret;

        let fut = async move {
            cross_log!(info, "Spawning auth signup flow polling task");
            PubkySignupAuthFlow::poll_for_token_loop(bg_client, bg_relay, bg_secret, tx).await;
        };

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(Abortable::new(fut, abort_reg));

        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(Abortable::new(fut, abort_reg).map(|_| ()));

        Ok(PubkySignupAuthFlow {
            client,
            auth_url: self.url.to_url(),
            rx,
            abort: abort_handle,
        })
    }
}

/// Error type for [`SignupAuthUrl`] parsing and construction.
#[derive(Debug, thiserror::Error)]
pub enum SignupAuthUrlError {
    /// The scheme is invalid. Expected 'pubkyauth' but got '{0}'.
    #[error("Invalid scheme. Expected 'pubkyauth' but got '{0}'")]
    InvalidScheme(String),
    /// The path is invalid. Expected '/signup' but got '{0}'.
    #[error("Invalid path. Expected '/signup' but got '{0}'")]
    InvalidPath(String),
    /// The query is invalid. Missing or invalid query parameter '{0}'.
    #[error("Invalid query. Missing or invalid query parameter '{0}'")]
    InvalidQuery(String),
}

/// A Pubky Signup Auth URL that is used to start the qr signup flow.
/// Deeplink or QR code can be generated from this URL.
#[derive(Debug, Clone)]
pub struct SignupAuthUrl {
    invite_code: Option<String>,
    homeserver_public_key: PublicKey,
    caps: Capabilities,
    relay: Url,
    client_secret: [u8; 32],
}

impl SignupAuthUrl {
    /// Create a new `SignupAuthUrl` with the given homeserver public key and capabilities.
    /// Uses the default HTTP relay.
    #[allow(
        clippy::missing_panics_doc,
        reason = "Should be able to parse the default HTTP relay"
    )]
    #[must_use]
    pub fn new(homeserver_public_key: PublicKey, caps: Capabilities) -> Self {
        Self {
            invite_code: None,
            homeserver_public_key,
            caps,
            relay: Url::parse(DEFAULT_HTTP_RELAY)
                .expect("Should be able to parse the default HTTP relay"),
            client_secret: random_bytes::<32>(),
        }
    }

    /// Get the invite code for the signup flow.
    #[must_use]
    pub fn invite_code(&self) -> Option<&String> {
        self.invite_code.as_ref()
    }

    /// Get the homeserver public key for the signup flow.
    #[must_use]
    pub fn homeserver_public_key(&self) -> &PublicKey {
        &self.homeserver_public_key
    }

    /// Get the capabilities for the signup flow.
    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// Get the relay for the signup flow.
    #[must_use]
    pub fn relay(&self) -> &Url {
        &self.relay
    }

    /// Get the client secret for the signup flow.
    #[must_use]
    pub fn client_secret(&self) -> &[u8; 32] {
        &self.client_secret
    }

    /// Set the invite code for the signup flow.
    pub fn set_invite_code(&mut self, invite_code: Option<String>) {
        self.invite_code = invite_code;
    }

    /// Set a custom relay for the signup flow.
    pub fn set_relay(&mut self, relay: Url) {
        self.relay = relay;
    }

    /// Convert the `SignupAuthUrl` to a URL.
    #[allow(
        clippy::missing_panics_doc,
        reason = "Should be able to parse the base url"
    )]
    #[must_use]
    pub fn to_url(&self) -> Url {
        let mut url =
            Url::parse("pubkyauth:///signup").expect("Should be able to parse the signup URL");
        let mut query = url.query_pairs_mut();
        query.append_pair("caps", &self.caps.to_string());
        query.append_pair("secret", &URL_SAFE_NO_PAD.encode(self.client_secret));
        query.append_pair("relay", self.relay.as_str());
        query.append_pair("hs", &self.homeserver_public_key.to_string());
        if let Some(invite_code) = &self.invite_code {
            query.append_pair("ic", invite_code);
        }
        drop(query);
        url
    }

    /// Parse a URL into a `SignupAuthUrl`.
    /// Returns an error if the URL is invalid not matching the expected format.
    /// # Errors
    /// - Returns [`SignupAuthUrlError::InvalidScheme`] if the scheme is not "pubkyauth://".
    /// - Returns [`SignupAuthUrlError::InvalidPath`] if the path is not "/signup".
    /// - Returns [`SignupAuthUrlError::InvalidQuery`] if a required query parameter is missing or invalid.
    pub fn parse_url(url: &Url) -> std::result::Result<Self, SignupAuthUrlError> {
        fn extract_query_param(
            url: &Url,
            param: &str,
        ) -> std::result::Result<String, SignupAuthUrlError> {
            url.query_pairs()
                .find(|(k, _)| k == param)
                .map(|(_, v)| v.to_string())
                .ok_or(SignupAuthUrlError::InvalidQuery(param.to_string()))
        }

        if url.scheme().to_lowercase() != "pubkyauth" {
            return Err(SignupAuthUrlError::InvalidScheme(url.scheme().to_string()));
        }
        if url.path() != "/signup" {
            return Err(SignupAuthUrlError::InvalidPath(url.path().to_string()));
        }

        let caps = extract_query_param(url, "caps")?;
        let caps: Capabilities = Capabilities::try_from(caps.as_str())
            .map_err(|e| SignupAuthUrlError::InvalidQuery(format!("Invalid caps: {e}")))?;
        let secret = extract_query_param(url, "secret")?;
        let client_secret = URL_SAFE_NO_PAD
            .decode(secret)
            .map_err(|e| SignupAuthUrlError::InvalidQuery(format!("Invalid secret: {e}")))?;
        let client_secret: [u8; 32] = client_secret
            .try_into()
            .map_err(|_e| SignupAuthUrlError::InvalidQuery("Invalid client secret".to_string()))?;
        let relay = extract_query_param(url, "relay")?;
        let relay = Url::parse(&relay)
            .map_err(|e| SignupAuthUrlError::InvalidQuery(format!("Invalid relay: {e}")))?;
        let homeserver_public_key = extract_query_param(url, "hs")?;
        let homeserver_public_key: PublicKey = homeserver_public_key.parse().map_err(|e| {
            SignupAuthUrlError::InvalidQuery(format!("Invalid homeserver public key: {e}"))
        })?;
        let ic = if url.query_pairs().any(|(k, _)| k == "ic") {
            let invite_code = extract_query_param(url, "ic")?;
            Some(invite_code)
        } else {
            None
        };

        Ok(SignupAuthUrl {
            invite_code: ic,
            homeserver_public_key,
            caps,
            relay,
            client_secret,
        })
    }
}

impl TryInto<SignupAuthUrl> for Url {
    type Error = SignupAuthUrlError;

    fn try_into(self) -> std::result::Result<SignupAuthUrl, Self::Error> {
        SignupAuthUrl::parse_url(&self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_url() {
        let homeserver_public_key: PublicKey =
            "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
                .parse()
                .unwrap();
        let caps = Capabilities::builder().read_write("/").finish();

        let mut signup_url = SignupAuthUrl::new(homeserver_public_key.clone(), caps.clone());
        signup_url.set_invite_code(Some("1234567890".to_string()));
        let url = signup_url.to_url();
        let parsed_url = SignupAuthUrl::parse_url(&url).unwrap();
        assert_eq!(parsed_url.caps, caps);
        assert_eq!(parsed_url.homeserver_public_key, homeserver_public_key);
        assert_eq!(parsed_url.invite_code, Some("1234567890".to_string()));
    }

    #[tokio::test]
    async fn constructs_urls_and_channel() {
        let homeserver_public_key: PublicKey =
            "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
                .parse()
                .unwrap();
        let caps = Capabilities::builder().read_write("/").finish();
        let invite_code = "1234567890";
        let flow = PubkySignupAuthFlow::builder(&caps, homeserver_public_key.clone())
            .invite_code(invite_code.to_string())
            .start()
            .unwrap();

        // pubkyauth:///signup deep link contains caps + secret + relay + hs + ic
        let url = flow.authorization_url();
        assert!(url.as_str().starts_with("pubkyauth:///signup?"));
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "caps" && v == caps.to_string())
        );
        assert!(url.query_pairs().any(|(k, _)| k == "secret"));
        assert!(url.query_pairs().any(|(k, _)| k == "relay"));
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "hs" && v == homeserver_public_key.to_string())
        );
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "ic" && v == invite_code)
        );
    }
}
