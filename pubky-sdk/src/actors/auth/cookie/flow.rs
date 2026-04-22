//! Legacy cookie auth flow — QR/deeplink → signer approval → cookie session.
//!
//! ## Sign in
//! ```no_run
//! # use pubky::{Capabilities, PubkyCookieAuthFlow, AuthFlowKind};
//! # async fn run() -> pubky::Result<()> {
//! # #[allow(deprecated)] {
//! let caps = Capabilities::default();
//! let flow = PubkyCookieAuthFlow::start(&caps, AuthFlowKind::signin())?;
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # }
//! # Ok(()) }
//! ```
//!
//! ## Sign in (credential-level, for persistence or inspection)
//! ```no_run
//! # use pubky::{Capabilities, PubkyCookieAuthFlow, AuthFlowKind, PubkyHttpClient, PubkySession};
//! # async fn run() -> pubky::Result<()> {
//! # #[allow(deprecated)] {
//! let client = PubkyHttpClient::new()?;
//! let flow = PubkyCookieAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin())
//!     .client(client.clone())
//!     .start()?;
//! let credential = flow.await_credential().await?;
//! // ... store or inspect the credential ...
//! let session = PubkySession::from_cookie_credential(client, credential);
//! # }
//! # Ok(()) }
//! ```
//!
//! ## Sign up
//! ```no_run
//! # use pubky::{Capabilities, PubkyCookieAuthFlow, AuthFlowKind, PublicKey};
//! # async fn run() -> pubky::Result<()> {
//! # #[allow(deprecated)] {
//! let caps = Capabilities::default();
//! let homeserver: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let flow = PubkyCookieAuthFlow::start(
//!     &caps,
//!     AuthFlowKind::signup(homeserver, Some("token".into())),
//! )?;
//! let session = flow.await_approval().await?;
//! # }
//! # Ok(()) }
//! ```

use url::Url;

use crate::actors::auth::cookie::approval::CookieApproval;
use crate::actors::auth::cookie::builder::CookieAuthFlowBuilder;
use crate::actors::auth::cookie::credential::CookieCredential;
use crate::actors::auth::deep_links::DeepLink;
use crate::actors::auth::kind::AuthFlowKind;
use crate::actors::auth::relay::auth_relay_listener::AuthRelayListener;
use crate::errors::Result;
#[allow(deprecated, reason = "Internal use of deprecated public API")]
use crate::AuthToken;
use crate::{Capabilities, PubkyHttpClient, PubkySession};

/// End-to-end **legacy (cookie) auth flow** handle.
///
/// 1. Construct with [`PubkyCookieAuthFlow::start`] or
///    [`PubkyCookieAuthFlow::builder`].
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to
///    the signer.
/// 3. Complete the flow with [`await_approval`](Self::await_approval) for a
///    ready [`PubkySession`], or [`await_credential`](Self::await_credential)
///    for a raw [`CookieCredential`]. Non-blocking companions:
///    [`try_poll_once`](Self::try_poll_once),
///    [`try_poll_credential_once`](Self::try_poll_credential_once).
///
/// Background polling **starts immediately** at construction. Dropping this
/// value cancels the background task; the relay channel itself expires
/// server-side after its TTL.
#[deprecated(
    note = "Use PubkyJwtAuthFlow instead. Cookie-backed sessions are being phased out in favor of JWT-backed, self-refreshing sessions."
)]
#[derive(Debug)]
pub struct PubkyCookieAuthFlow {
    relay_listener: AuthRelayListener,
    client: PubkyHttpClient,
    auth_url: DeepLink,
}

#[allow(deprecated, reason = "Internal use of deprecated public API")]
impl PubkyCookieAuthFlow {
    pub(crate) fn new(
        relay_listener: AuthRelayListener,
        client: PubkyHttpClient,
        auth_url: DeepLink,
    ) -> Self {
        Self {
            relay_listener,
            client,
            auth_url,
        }
    }

    /// Start a cookie flow with the default HTTP relay.
    ///
    /// Spawns the background poller immediately and returns a handle.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing
    ///   [`PubkyHttpClient`] or generating the relay URL fails.
    pub fn start(caps: &Capabilities, auth_kind: AuthFlowKind) -> Result<Self> {
        CookieAuthFlowBuilder::new(caps.clone(), auth_kind).start()
    }

    /// Create a builder to override the **relay** and/or provide a custom
    /// **client**.
    #[must_use]
    pub fn builder(caps: &Capabilities, auth_kind: AuthFlowKind) -> CookieAuthFlowBuilder {
        CookieAuthFlowBuilder::new(caps.clone(), auth_kind)
    }

    /// The `pubkyauth://` deep link you display (QR/URL) to the signer.
    #[must_use]
    pub fn authorization_url(&self) -> Url {
        self.auth_url.clone().into()
    }

    /// Block until the signer approves and return a ready-to-use
    /// [`PubkySession`].
    ///
    /// Composes [`await_credential`](Self::await_credential) +
    /// [`PubkySession::from_cookie_credential`]. Use
    /// [`await_credential`](Self::await_credential) directly if you need to
    /// inspect or persist the credential before building a session.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay or
    ///   exchanging the token at `/session`.
    pub async fn await_approval(self) -> Result<PubkySession> {
        let client = self.client.clone();
        let credential = self.await_credential().await?;
        Ok(PubkySession::from_cookie_credential(client, credential))
    }

    /// Block until the signer approves and the homeserver issues a
    /// [`CookieCredential`].
    ///
    /// The credential can be inspected, persisted, or lifted into a full
    /// [`PubkySession`] via [`PubkySession::from_cookie_credential`].
    ///
    /// # Errors
    /// - See [`await_approval`](Self::await_approval).
    pub async fn await_credential(self) -> Result<CookieCredential> {
        let Self {
            relay_listener,
            client,
            ..
        } = self;
        let approval = Self::await_decoded_approval(relay_listener).await?;
        CookieCredential::from_auth_token(&approval.0, &client).await
    }

    /// Block until the signer approves and we receive an [`AuthToken`].
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expires before approval.
    /// - Propagates HTTP/transport failures encountered while polling the relay.
    pub async fn await_token(self) -> Result<AuthToken> {
        let approval = Self::await_decoded_approval(self.relay_listener).await?;
        Ok(approval.0)
    }

    /// Non-blocking probe (single step) that **consumes any ready token** and returns:
    /// - `Ok(Some(session))` when a token was delivered and the session established.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expired before a token arrived.
    /// - Propagates HTTP/transport failures from constructing the session.
    pub async fn try_poll_once(&self) -> Result<Option<PubkySession>> {
        let Some(credential) = self.try_poll_credential_once().await? else {
            return Ok(None);
        };
        Ok(Some(PubkySession::from_cookie_credential(
            self.client.clone(),
            credential,
        )))
    }

    /// Non-blocking variant of [`await_credential`](Self::await_credential).
    ///
    /// Returns `Ok(Some(credential))` when a token has been delivered and the
    /// homeserver has issued a credential; `Ok(None)` if no payload yet;
    /// `Err` on transport/server errors.
    ///
    /// # Errors
    /// - See [`try_poll_once`](Self::try_poll_once).
    pub async fn try_poll_credential_once(&self) -> Result<Option<CookieCredential>> {
        let Some(approval) = self.try_decoded_approval()? else {
            return Ok(None);
        };
        Ok(Some(
            CookieCredential::from_auth_token(&approval.0, &self.client).await?,
        ))
    }

    /// Non-blocking check: returns a verified `AuthToken` if the background
    /// poller has delivered it.
    ///
    /// - `Some(Ok(AuthToken))` when ready.
    /// - `Some(Err(_))` if the background task failed (expired/transport error).
    /// - `None` if not yet delivered.
    #[must_use]
    pub fn try_token(&self) -> Option<Result<AuthToken>> {
        match self.try_decoded_approval() {
            Ok(Some(approval)) => Some(Ok(approval.0)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }

    async fn await_decoded_approval(
        relay_listener: AuthRelayListener,
    ) -> Result<CookieApproval> {
        let message = relay_listener.await_message().await?;
        CookieApproval::decode(&message)
    }

    fn try_decoded_approval(&self) -> Result<Option<CookieApproval>> {
        let Some(message) = self.relay_listener.try_message() else {
            return Ok(None);
        };
        Ok(Some(CookieApproval::decode(&message?)?))
    }
}
