//! Client <=> Signer authing (“pubkyauth”) as a single, self-contained flow.
//!
//! ## TL;DR (happy path)
//! 
//! ### Sign in
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let flow = PubkyAuthFlow::start(&caps, AuthFlowKind::sign_in())?; // starts background polling immediately
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! // Blocks until the signer (e.g., Pubky Ring) approves and server issues a session.
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//! 
//! ### Sign up
//! ```no_run
//! # use pubky::{Capabilities, PubkyAuthFlow, AuthFlowKind};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let homeserver_public_key: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let signup_token = "1234567890";
//! let flow = PubkyAuthFlow::start(&caps, AuthFlowKind::signup(homeserver_public_key, Some(signup_token.to_string())))?; // starts background polling immediately
//! println!("Scan to sign up: {}", flow.authorization_url());
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

use pkarr::PublicKey;
use url::Url;

use pubky_common::crypto::random_bytes;

use crate::{
    AuthToken, Capabilities, PubkyHttpClient, PubkySession,
    actors::{DEFAULT_HTTP_RELAY, auth_permission_subscription::AuthPermissionSubscription},
    errors::Result,
};

#[cfg(target_arch = "wasm32")]
use futures_util::FutureExt; // for `.map(|_| ())` in WASM spawn

/// End-to-end **auth flow** (request + live polling) you *hold on to*.
/// 
/// Supports both sign in and sign up flows.
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
    pub fn start(caps: &Capabilities, auth_kind: AuthFlowKind) -> Result<Self> {
        PubkyAuthFlowBuilder::new(caps.clone(), auth_kind).start()
    }

    /// Create a builder to override **relay** and/or provide a custom **client**.
    #[must_use]
    pub fn builder(caps: &Capabilities, auth_kind: AuthFlowKind) -> PubkyAuthFlowBuilder {
        PubkyAuthFlowBuilder::new(caps.clone(), auth_kind)
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

/// The kind of authentication flow to perform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFlowKind {
    /// Sign in to an existing account.
    SignIn,
    /// Sign up for a new account.
    SignUp {
        /// The public key of the homeserver to sign up on.
        homeserver_public_key: PublicKey,
        /// The signup token to use for the signup flow.
        /// This is optional.
        signup_token: Option<String>,
    },
}

impl AuthFlowKind {
    /// Create a sign in flow.
    pub fn sign_in() -> Self {
        Self::SignIn
    }

    /// Create a sign up flow.
    /// # Arguments
    /// * `homeserver_public_key` - The public key of the homeserver to sign up on.
    /// * `signup_token` - The signup token to use for the signup flow. This is optional.
    pub fn sign_up(homeserver_public_key: PublicKey, signup_token: Option<String>) -> Self {
        Self::SignUp {
            homeserver_public_key,
            signup_token,
        }
    }
}

/// Builder for [`PubkyAuthFlow`].
///
/// Use to override the HTTP relay and/or the `PubkyHttpClient`.
#[derive(Debug, Clone)]
pub struct PubkyAuthFlowBuilder {
    caps: Capabilities,
    base_relay: Url,
    client: Option<PubkyHttpClient>,
    auth_kind: AuthFlowKind,
    client_secret: [u8; 32],
}

impl PubkyAuthFlowBuilder {
    /// Create a new builder for the auth flow.
    /// # Arguments
    /// * `caps` - The capabilities to use for the auth flow.
    /// * `auth_kind` - The kind of auth flow to perform.
    /// # Returns
    /// A new builder for the auth flow.
    pub(crate) fn new(caps: Capabilities, auth_kind: AuthFlowKind) -> Self {
        Self {
            caps,
            base_relay: Url::parse(DEFAULT_HTTP_RELAY)
                .expect("Should be able to parse the default HTTP relay"),
            client: None,
            auth_kind,
            client_secret: random_bytes::<32>(),
        }
    }

    /// Set a custom relay base URL. The flow will append the per-channel segment
    /// as `base + base64url(hash(client_secret))`. Trailing slash optional.
    pub fn base_relay(mut self, url: Url) -> Self {
        self.base_relay = url;
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the client secret to use for the auth flow.
    /// By default, a random client secret is generated.
    pub fn client_secret(mut self, client_secret: [u8; 32]) -> Self {
        self.client_secret = client_secret;
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn the background poller,
    /// and return the flow handle.
    pub fn start(self) -> Result<PubkyAuthFlow> {
        let client = match &self.client {
            Some(c) => c.clone(),
            None => PubkyHttpClient::new()?,
        };

        let auth_url = self.create_url();

        let subscription = AuthPermissionSubscription::builder(self.client_secret)
            .relay_base_url(self.base_relay)
            .client(client)
            .start()?;

        Ok(PubkyAuthFlow {
            subscription,
            auth_url,
        })
    }

    /// Create the auth URL for the auth flow.
    /// Depending on the auth kind, the URL will be different
    fn create_url(&self) -> Url {
        let mut auth_url = match &self.auth_kind {
            AuthFlowKind::SignIn => {
                Url::parse("pubkyauth:///").expect("Should be able to parse the base url")
            }
            AuthFlowKind::SignUp { .. } => {
                Url::parse("pubkyauth:///signup").expect("Should be able to parse the base url")
            }
        };

        {
            // Add common parameters for both signin and signup flows.
            let mut query = auth_url.query_pairs_mut();
            query.append_pair("caps", &self.caps.to_string());
            query.append_pair("secret", &URL_SAFE_NO_PAD.encode(&self.client_secret));
            query.append_pair("relay", self.base_relay.as_str());
        }

        // Add signup parameters if it is a signup flow.
        if let AuthFlowKind::SignUp {
            homeserver_public_key,
            signup_token,
        } = &self.auth_kind
        {
            let mut query = auth_url.query_pairs_mut();
            query.append_pair("hs", &homeserver_public_key.to_string());
            if let Some(signup_token) = signup_token {
                query.append_pair("st", signup_token);
            }
        }

        auth_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn construct_signin_url() {
        let caps = Capabilities::default();
        let flow = PubkyAuthFlow::start(&caps, AuthFlowKind::SignIn).unwrap();

        // pubkyauth:// deep link contains caps + secret + relay
        let url = flow.authorization_url();
        assert!(url.as_str().starts_with("pubkyauth:///?"));
        assert!(
            url.query_pairs()
                .any(|(k, v)| k == "caps" && v == caps.to_string())
        );
        assert!(url.query_pairs().any(|(k, _)| k == "secret"));
        assert!(url.query_pairs().any(|(k, _)| k == "relay"));
    }

    #[tokio::test]
    async fn construct_signup_url() {
        let homeserver_public_key: PublicKey =
            "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
                .parse()
                .unwrap();
        let caps = Capabilities::builder().read_write("/").finish();
        let signup_token = "1234567890";
        let flow = PubkyAuthFlow::start(
            &caps,
            AuthFlowKind::sign_up(
                homeserver_public_key.clone(),
                Some(signup_token.to_string()),
            ),
        )
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
 
    }
}
