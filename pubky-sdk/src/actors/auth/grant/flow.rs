//! Grant + `PoP` auth flow — QR/deeplink → signer approval → self-refreshing session.
//!
//! ## Sign in
//! ```no_run
//! # use pubky::{Capabilities, PubkyGrantAuthFlow, AuthFlowKind, ClientId};
//! # async fn run() -> pubky::Result<()> {
//! let caps = Capabilities::default();
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyGrantAuthFlow::start(&caps, AuthFlowKind::signin(), client_id)?;
//! println!("Scan to sign in: {}", flow.authorization_url());
//!
//! let session = flow.await_approval().await?;
//! println!("Signed in as {}", session.info().public_key());
//! # Ok(()) }
//! ```
//!
//! ## Sign in (credential-level, for persistence or inspection)
//! ```no_run
//! # use pubky::{Capabilities, PubkyGrantAuthFlow, AuthFlowKind, ClientId, PubkyHttpClient, PubkySession};
//! # async fn run() -> pubky::Result<()> {
//! let client = PubkyHttpClient::new()?;
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyGrantAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin(), client_id)
//!     .client(client.clone())
//!     .start()?;
//! let credential = flow.await_credential().await?;
//! // ... store or inspect the credential ...
//! let session = PubkySession::from_grant_credential(client, credential);
//! # Ok(()) }
//! ```
//!
//! ## Custom relay / non-blocking UI
//! ```no_run
//! # use pubky::{Capabilities, PubkyGrantAuthFlow, AuthFlowKind, ClientId};
//! # use std::time::Duration;
//! # async fn ui() -> pubky::Result<()> {
//! let client_id = ClientId::new("my.app").unwrap();
//! let flow = PubkyGrantAuthFlow::builder(&Capabilities::default(), AuthFlowKind::signin(), client_id)
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

use std::{fmt, str::FromStr};

use pubky_common::{
    auth::jws::ClientId,
    crypto::{Keypair, PublicKey},
};
use url::Url;

use crate::actors::Pkdns;
use crate::actors::auth::deep_links::DeepLink;
use crate::actors::auth::grant::approval::GrantApproval;
use crate::actors::auth::grant::builder::GrantAuthFlowBuilder;
use crate::actors::auth::grant::credential::GrantCredential;
use crate::actors::auth::grant::grant_exchange::credential_from_grant_exchange;
use crate::actors::auth::kind::AuthFlowKind;
use crate::actors::auth::relay::auth_relay_listener::AuthRelayListener;
use crate::errors::{AuthError, Result};
use crate::{Capabilities, PubkyHttpClient, PubkySession};

/// Serializable state for resuming a pending grant auth flow.
///
/// This is not a session credential. It only preserves enough local state to
/// continue polling an unapproved grant auth flow after the original
/// [`PubkyGrantAuthFlow`] handle was dropped. Treat it as sensitive temporary
/// data: it contains the relay secret in [`Self::authorization_url`] and the
/// `PoP` client private key in [`Self::client_key_secret`].
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct GrantAuthFlowState {
    /// Original grant authorization URL shown to the signer.
    pub authorization_url: String,
    /// Secret bytes for the `PoP` client keypair bound by the deep link `cpk`.
    pub client_key_secret: [u8; 32],
}

impl fmt::Debug for GrantAuthFlowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrantAuthFlowState")
            .field("authorization_url", &self.authorization_url)
            .field("client_key_secret", &"<redacted>")
            .finish()
    }
}

/// End-to-end **Grant + `PoP` auth flow** handle.
///
/// 1. Construct with [`PubkyGrantAuthFlow::start`] or
///    [`PubkyGrantAuthFlow::builder`].
/// 2. Display [`authorization_url`](Self::authorization_url) (QR/deeplink) to
///    the signer.
/// 3. Complete with [`await_approval`](Self::await_approval) for a ready
///    [`PubkySession`], or [`await_credential`](Self::await_credential) for
///    a raw [`GrantCredential`]. Non-blocking companions:
///    [`try_poll_once`](Self::try_poll_once),
///    [`try_poll_credential_once`](Self::try_poll_credential_once).
///
/// Background polling **starts immediately** at construction. Dropping this
/// value cancels the background task; the relay channel itself expires
/// server-side after its TTL.
pub struct PubkyGrantAuthFlow {
    relay_listener: AuthRelayListener,
    client: PubkyHttpClient,
    auth_url: DeepLink,
    client_keypair: Keypair,
}

impl fmt::Debug for PubkyGrantAuthFlow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PubkyGrantAuthFlow")
            .field("relay_listener", &self.relay_listener)
            .field("client", &self.client)
            .field("auth_url", &self.auth_url)
            .field("client_keypair", &"<redacted>")
            .finish()
    }
}

impl PubkyGrantAuthFlow {
    pub(crate) fn new(
        relay_listener: AuthRelayListener,
        client: PubkyHttpClient,
        auth_url: DeepLink,
        client_keypair: Keypair,
    ) -> Self {
        Self {
            relay_listener,
            client,
            auth_url,
            client_keypair,
        }
    }

    /// Start a grant flow with the default HTTP relay.
    ///
    /// The resulting [`PubkySession`] is grant-backed and self-refreshes.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error`] if constructing the backing
    ///   [`PubkyHttpClient`] or generating the relay URL fails.
    pub fn start(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> Result<Self> {
        GrantAuthFlowBuilder::new(caps.clone(), auth_kind, client_id).start()
    }

    /// Create a builder to override the **relay**, provide a custom **client**,
    /// or pin a specific **`PoP` keypair**.
    #[must_use]
    pub fn builder(
        caps: &Capabilities,
        auth_kind: AuthFlowKind,
        client_id: ClientId,
    ) -> GrantAuthFlowBuilder {
        GrantAuthFlowBuilder::new(caps.clone(), auth_kind, client_id)
    }

    /// The `pubkyauth://` deep link you display (QR/URL) to the signer.
    #[must_use]
    pub fn authorization_url(&self) -> Url {
        self.auth_url.clone().into()
    }

    /// Save the sensitive state required to restore this pending grant flow.
    ///
    /// The returned state is only useful while the relay inbox still exists.
    /// It should be stored temporarily and deleted once the flow completes,
    /// expires, or is abandoned.
    #[must_use]
    pub fn save(&self) -> GrantAuthFlowState {
        GrantAuthFlowState {
            authorization_url: self.authorization_url().to_string(),
            client_key_secret: self.client_keypair.secret(),
        }
    }

    /// Restore a pending grant auth flow from state produced by [`Self::save`].
    ///
    /// This re-subscribes to the relay channel encoded in the authorization URL
    /// and validates that the saved `PoP` client key matches the `cpk` in the
    /// grant deep link.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the saved URL is
    ///   not a grant auth deep link or the saved client key does not match it.
    /// - Propagates failures from starting the relay listener.
    pub fn restore(state: GrantAuthFlowState, client: PubkyHttpClient) -> Result<Self> {
        let GrantAuthFlowState {
            authorization_url,
            client_key_secret,
        } = state;
        let auth_url = DeepLink::from_str(&authorization_url).map_err(|e| {
            AuthError::Validation(format!("failed to parse grant auth flow state URL: {e}"))
        })?;
        let (relay, secret, client_pk) = grant_deep_link_parts(&auth_url)?;
        let client_keypair = Keypair::from_secret(&client_key_secret);

        if &client_keypair.public_key() != client_pk {
            return Err(AuthError::Validation(
                "saved grant auth flow client key does not match the deep link client public key"
                    .into(),
            )
            .into());
        }

        let relay_listener = AuthRelayListener::builder(*secret)
            .relay_base_url(relay.clone())
            .client(client.clone())
            .start()?;

        Ok(Self::new(relay_listener, client, auth_url, client_keypair))
    }

    /// Block until the signer approves and return a ready-to-use
    /// [`PubkySession`].
    ///
    /// Composes [`await_credential`](Self::await_credential) +
    /// [`PubkySession::from_grant_credential`]. Use
    /// [`await_credential`](Self::await_credential) directly if you need to
    /// inspect or persist the credential before building a session.
    ///
    /// # Errors
    /// - Returns [`crate::errors::Error::Authentication`] if the relay channel
    ///   expires before approval.
    /// - Propagates HTTP/transport failures while polling the relay or
    ///   exchanging the grant for a bearer.
    /// - Returns [`crate::errors::Error::Authentication`] if the issuer's
    ///   homeserver cannot be resolved via PKARR (sign-in only).
    pub async fn await_approval(self) -> Result<PubkySession> {
        let client = self.client.clone();
        let credential = self.await_credential().await?;
        Ok(PubkySession::from_grant_credential(client, credential))
    }

    /// Block until the signer approves and the homeserver issues a
    /// [`GrantCredential`].
    ///
    /// The credential can be inspected, persisted, or lifted into a full
    /// [`PubkySession`] via [`PubkySession::from_grant_credential`].
    ///
    /// # Errors
    /// - See [`await_approval`](Self::await_approval).
    pub async fn await_credential(self) -> Result<GrantCredential> {
        let Self {
            relay_listener,
            client,
            client_keypair,
            ..
        } = self;
        let approval = Self::await_decoded_approval(relay_listener).await?;
        Self::exchange_for_credential(&client, approval, client_keypair).await
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
        Ok(Some(PubkySession::from_grant_credential(
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
    pub async fn try_poll_credential_once(&self) -> Result<Option<GrantCredential>> {
        let Some(approval) = self.try_decoded_approval()? else {
            return Ok(None);
        };
        let credential =
            Self::exchange_for_credential(&self.client, approval, self.client_keypair.clone())
                .await?;
        Ok(Some(credential))
    }

    async fn exchange_for_credential(
        client: &PubkyHttpClient,
        approval: GrantApproval,
        client_keypair: Keypair,
    ) -> Result<GrantCredential> {
        let GrantApproval { jws, claims } = approval;

        let pkdns = Pkdns::with_client(client.clone());
        let hs_pk = pkdns.get_homeserver_of(&claims.iss).await.ok_or_else(|| {
            AuthError::Validation(format!(
                "could not resolve homeserver for {}",
                claims.iss.z32()
            ))
        })?;
        credential_from_grant_exchange(client, jws, claims, client_keypair, hs_pk).await
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

fn grant_deep_link_parts(deep_link: &DeepLink) -> Result<(&Url, &[u8; 32], &PublicKey)> {
    match deep_link {
        DeepLink::SigninGrant(link) => Ok((link.relay(), link.secret(), link.client_pk())),
        DeepLink::SignupGrant(link) => Ok((link.relay(), link.secret(), link.client_pk())),
        _ => Err(AuthError::Validation(
            "saved grant auth flow state must contain a grant signin or signup deep link".into(),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::auth::deep_links::{SigninDeepLink, SigninGrantDeepLink};

    #[tokio::test]
    async fn save_restore_round_trips_authorization_url() {
        let relay = http_relay::HttpRelay::builder()
            .http_port(0)
            .run()
            .await
            .unwrap();
        let relay_url = relay.local_url().join("inbox").unwrap();
        let client = PubkyHttpClient::new().unwrap();
        let client_id = ClientId::new("save-restore.test").unwrap();
        let flow = PubkyGrantAuthFlow::builder(
            &Capabilities::default(),
            AuthFlowKind::signin(),
            client_id,
        )
        .relay(relay_url)
        .client(client.clone())
        .start()
        .unwrap();

        let restored = PubkyGrantAuthFlow::restore(flow.save(), client).unwrap();

        assert_eq!(restored.authorization_url(), flow.authorization_url());
    }

    #[test]
    fn restore_rejects_cookie_auth_url() {
        let auth_url = SigninDeepLink::new(
            Capabilities::default(),
            Url::parse("http://localhost/inbox").unwrap(),
            [7; 32],
        )
        .to_string();
        let state = GrantAuthFlowState {
            authorization_url: auth_url,
            client_key_secret: Keypair::random().secret(),
        };

        let error = PubkyGrantAuthFlow::restore(state, PubkyHttpClient::new().unwrap())
            .unwrap_err()
            .to_string();

        assert!(error.contains("grant signin or signup deep link"));
    }

    #[test]
    fn restore_rejects_mismatched_client_key() {
        let expected_client = Keypair::random();
        let actual_client = Keypair::random();
        let auth_url = SigninGrantDeepLink::new(
            Capabilities::default(),
            Url::parse("http://localhost/inbox").unwrap(),
            [7; 32],
            ClientId::new("mismatch.test").unwrap(),
            expected_client.public_key(),
        )
        .to_string();
        let state = GrantAuthFlowState {
            authorization_url: auth_url,
            client_key_secret: actual_client.secret(),
        };

        let error = PubkyGrantAuthFlow::restore(state, PubkyHttpClient::new().unwrap())
            .unwrap_err()
            .to_string();

        assert!(error.contains("does not match"));
    }

    #[cfg(feature = "json")]
    #[test]
    fn state_serializes_round_trip() {
        let state = GrantAuthFlowState {
            authorization_url: "pubkyauth://signin?caps=&relay=http://localhost/inbox".into(),
            client_key_secret: [42; 32],
        };

        let json = serde_json::to_string(&state).unwrap();
        let restored: GrantAuthFlowState = serde_json::from_str(&json).unwrap();

        assert_eq!(restored, state);
    }
}
