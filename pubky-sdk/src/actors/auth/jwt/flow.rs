//! JWT (grant + `PoP`) auth flow — QR/deeplink → signer approval → self-refreshing session.
//!
//! ## Sign in
//! ```no_run
//! # use pubky::{Capabilities, PubkyJwtAuthFlow, AuthFlowKind, ClientId};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyJwtAuthFlow::start(&caps, AuthFlowKind::signin(), client_id)?;
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ## Sign in (credential-level, for persistence or inspection)
//! ```no_run
//! # use pubky::{Capabilities, PubkyJwtAuthFlow, AuthFlowKind, ClientId, PubkyHttpClient, PubkySession};
//! # async fn run() -> pubky::Result<()> {
//! let client = PubkyHttpClient::new()?;
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyJwtAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin(), client_id)
//!     .client(client.clone())
//!     .start()?;
//! let credential = flow.await_credential().await?;
//! // ... store or inspect the credential ...
//! let session = PubkySession::from_jwt_credential(client, credential);
//! # Ok(()) }
//! ```
//!
//! ## Sign up
//! ```no_run
//! # use pubky::{Capabilities, PubkyJwtAuthFlow, AuthFlowKind, ClientId, PublicKey};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let client_id = ClientId::new("my.app").unwrap();
//! let homeserver: PublicKey = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo".parse().unwrap();
//! let flow = PubkyJwtAuthFlow::start(
//!     &caps,
//!     AuthFlowKind::signup(homeserver, Some("token".into())),
//!     client_id,
//! )?;
//! let session = flow.await_approval().await?;
//! # Ok(()) }
//! ```
//!
//! ## Custom relay / non-blocking UI
//! ```no_run
//! # use pubky::{Capabilities, PubkyJwtAuthFlow, AuthFlowKind, ClientId};
//! # use std::time::Duration;
//! # async fn ui() -> pubky::Result<()> {
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyJwtAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin(), client_id)
//!     .relay(url::Url::parse("http://localhost:8080/inbox/")?)
//!     .start()?;
//!
//! loop {
//!     if let Some(_session) = flow.try_poll_once().await? {
//!         break;
//!     }
//!     tokio::time::sleep(Duration::from_millis(300)).await;
//! }
//! # Ok(()) }
//! ```

use std::fmt;

use pubky_common::{
    auth::jws::ClientId,
    crypto::{Keypair, PublicKey},
};
use url::Url;

use crate::actors::Pkdns;
use crate::actors::auth::deep_links::DeepLink;
use crate::actors::auth::jwt::approval::GrantApproval;
use crate::actors::auth::jwt::builder::JwtAuthFlowBuilder;
use crate::actors::auth::jwt::credential::JwtCredential;
use crate::actors::auth::jwt::grant_exchange::{
    credential_from_grant_exchange, credential_from_grant_signup,
};
use crate::actors::auth::kind::AuthFlowKind;
use crate::actors::auth::relay::auth_relay_listener::AuthRelayListener;
use crate::errors::{AuthError, Result};
use crate::{Capabilities, PubkyHttpClient, PubkySession};

/// End-to-end **JWT (grant + `PoP`) auth flow** handle.
///
/// 1. Construct with [`PubkyJwtAuthFlow::start`] or
///    [`PubkyJwtAuthFlow::builder`].
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to
///    the signer.
/// 3. Complete with [`await_approval`](Self::await_approval) for a ready
///    [`PubkySession`], or [`await_credential`](Self::await_credential) for
///    a raw [`JwtCredential`]. Non-blocking companions:
///    [`try_poll_once`](Self::try_poll_once),
///    [`try_poll_credential_once`](Self::try_poll_credential_once).
///
/// Background polling **starts immediately** at construction. Dropping this
/// value cancels the background task; the relay channel itself expires
/// server-side after its TTL.
pub struct PubkyJwtAuthFlow {
    relay_listener: AuthRelayListener,
    client: PubkyHttpClient,
    auth_url: DeepLink,
    client_keypair: Keypair,
    signup_homeserver: Option<(PublicKey, Option<String>)>,
}

impl fmt::Debug for PubkyJwtAuthFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PubkyJwtAuthFlow")
            .field("relay_listener", &self.relay_listener)
            .field("client", &self.client)
            .field("auth_url", &self.auth_url)
            .field("client_keypair", &"<redacted>")
            .field(
                "signup_homeserver",
                &self.signup_homeserver.as_ref().map(|(pk, _)| pk.z32()),
            )
            .finish()
    }
}

impl PubkyJwtAuthFlow {
    pub(crate) fn new(
        relay_listener: AuthRelayListener,
        client: PubkyHttpClient,
        auth_url: DeepLink,
        client_keypair: Keypair,
        signup_homeserver: Option<(PublicKey, Option<String>)>,
    ) -> Self {
        Self {
            relay_listener,
            client,
            auth_url,
            client_keypair,
            signup_homeserver,
        }
    }

    /// Start a JWT flow with the default HTTP relay.
    ///
    /// The resulting [`PubkySession`] is JWT-backed and self-refreshes.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing
    ///   [`PubkyHttpClient`] or generating the relay URL fails.
    pub fn start(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> Result<Self> {
        JwtAuthFlowBuilder::new(caps.clone(), auth_kind, client_id).start()
    }

    /// Create a builder to override the **relay**, provide a custom **client**,
    /// or pin a specific **`PoP` keypair**.
    #[must_use]
    pub fn builder(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> JwtAuthFlowBuilder {
        JwtAuthFlowBuilder::new(caps.clone(), auth_kind, client_id)
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
    /// [`PubkySession::from_jwt_credential`]. Use
    /// [`await_credential`](Self::await_credential) directly if you need to
    /// inspect or persist the credential before building a session.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay or
    ///   exchanging the grant for a JWT.
    /// - Returns [`crate::errors::Error::Authentication`] if the issuer's
    ///   homeserver cannot be resolved via PKARR (sign-in only).
    pub async fn await_approval(self) -> Result<PubkySession> {
        let client = self.client.clone();
        let credential = self.await_credential().await?;
        Ok(PubkySession::from_jwt_credential(client, credential))
    }

    /// Block until the signer approves and the homeserver issues a
    /// [`JwtCredential`].
    ///
    /// The credential can be inspected, persisted, or lifted into a full
    /// [`PubkySession`] via [`PubkySession::from_jwt_credential`].
    ///
    /// # Errors
    /// - See [`await_approval`](Self::await_approval).
    pub async fn await_credential(self) -> Result<JwtCredential> {
        let Self {
            relay_listener,
            client,
            client_keypair,
            signup_homeserver,
            ..
        } = self;
        let approval = Self::await_decoded_approval(relay_listener).await?;
        Self::exchange_for_credential(&client, approval, client_keypair, signup_homeserver).await
    }

    /// Non-blocking probe (single step) that **consumes any ready grant** and
    /// returns:
    /// - `Ok(Some(session))` when a grant was delivered and the session was
    ///   established at the homeserver.
    /// - `Ok(None)` if no payload yet (keep polling later).
    /// - `Err(e)` on transport/server errors or if the channel expired.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expired before a grant arrived.
    /// - Propagates HTTP/transport failures from establishing the session.
    pub async fn try_poll_once(&self) -> Result<Option<PubkySession>> {
        let Some(credential) = self.try_poll_credential_once().await? else {
            return Ok(None);
        };
        Ok(Some(PubkySession::from_jwt_credential(
            self.client.clone(),
            credential,
        )))
    }

    /// Non-blocking variant of [`await_credential`](Self::await_credential).
    ///
    /// Returns `Ok(Some(credential))` when a grant has been delivered and
    /// the homeserver has issued a credential; `Ok(None)` if no payload yet;
    /// `Err` on transport/server errors.
    ///
    /// # Errors
    /// - See [`try_poll_once`](Self::try_poll_once).
    pub async fn try_poll_credential_once(&self) -> Result<Option<JwtCredential>> {
        let Some(approval) = self.try_decoded_approval()? else {
            return Ok(None);
        };
        let credential = Self::exchange_for_credential(
            &self.client,
            approval,
            self.client_keypair.clone(),
            self.signup_homeserver.clone(),
        )
        .await?;
        Ok(Some(credential))
    }

    async fn exchange_for_credential(
        client: &PubkyHttpClient,
        approval: GrantApproval,
        client_keypair: Keypair,
        signup_homeserver: Option<(PublicKey, Option<String>)>,
    ) -> Result<JwtCredential> {
        let GrantApproval { jws, claims } = approval;

        if let Some((hs_pk, signup_token)) = signup_homeserver {
            credential_from_grant_signup(
                client,
                jws,
                claims,
                client_keypair,
                hs_pk,
                signup_token.as_deref(),
            )
            .await
        } else {
            let pkdns = Pkdns::with_client(client.clone());
            let hs_pk = pkdns.get_homeserver_of(&claims.iss).await.ok_or_else(|| {
                AuthError::Validation(format!(
                    "could not resolve homeserver for {}",
                    claims.iss.z32()
                ))
            })?;
            credential_from_grant_exchange(client, jws, claims, client_keypair, hs_pk).await
        }
    }

    async fn await_decoded_approval(relay_listener: AuthRelayListener) -> Result<GrantApproval> {
        let message = relay_listener.await_message().await?;
        GrantApproval::decode(&message)
    }

    fn try_decoded_approval(&self) -> Result<Option<GrantApproval>> {
        let Some(message) = self.relay_listener.try_message() else {
            return Ok(None);
        };
        Ok(Some(GrantApproval::decode(&message?)?))
    }
}
