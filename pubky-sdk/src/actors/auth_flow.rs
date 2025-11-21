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


use url::Url;

use pubky_common::crypto::random_bytes;

use crate::{
    AuthToken, Capabilities, PubkyHttpClient, PubkySession,
    actors::{DEFAULT_HTTP_RELAY, auth_permission_subscription::AuthPermissionSubscription},
    errors::{Error, Result},

};

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

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
#[derive(Debug)]
pub struct PubkyAuthFlow {
    subscription: AuthPermissionSubscription,
    auth_url: Url,
}

impl PubkyAuthFlow {
    /// Start a flow with the default HTTP relay.
    ///
    /// Spawns the background poller immediately and returns a handle.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing [`PubkyHttpClient`]
    ///   or generating the relay URL fails.
    pub fn start(caps: &Capabilities) -> Result<Self> {
        PubkyAuthFlowBuilder::new(caps.clone()).start()
    }

    /// Create a builder to override **relay** and/or provide a custom **client**.
    #[must_use]
    pub fn builder(caps: &Capabilities) -> PubkyAuthFlowBuilder {
        PubkyAuthFlowBuilder::new(caps.clone())
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
        self.subscription.await_approval().await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// This awaits the background poller’s result.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel expires before approval.
    /// - Propagates HTTP/transport failures encountered while polling the relay.
    pub async fn await_token(self) -> Result<AuthToken> {
        self.subscription.await_token().await
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
        self.subscription.try_poll_once().await
    }

    /// Non-blocking check: returns a verified `AuthToken` if the background poller has delivered it.
    ///
    /// - `Some(Ok(AuthToken))` when ready.
    /// - `Some(Err(_))` if the background task failed (expired/transport error).
    /// - `None` if not yet delivered.
    #[must_use]
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        self.subscription.try_token()
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
    pub(crate) const fn new(caps: Capabilities) -> Self {
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
        let relay = match self.relay {
            Some(u) => u,
            None => Url::parse(DEFAULT_HTTP_RELAY)?,
        };

        // 2) Generate client secret and build pubkyauth:// URL (caps + secret + relay).
        let client_secret = random_bytes::<32>();
        let caps_str = self.caps.to_string();
        let secret_b64 = URL_SAFE_NO_PAD.encode(client_secret);
        let relay_str = relay.as_str().to_owned();

        let mut auth_url = Url::parse("pubkyauth:///")?;
        {
            let mut query = auth_url.query_pairs_mut();
            query.append_pair("caps", &caps_str);
            query.append_pair("secret", &secret_b64);
            query.append_pair("relay", &relay_str)
        };

        let subscription = AuthPermissionSubscription::builder(client_secret)
        .relay_base_url(relay)
        .client(client)
        .start()?;

        Ok(PubkyAuthFlow {
            subscription,
            auth_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
