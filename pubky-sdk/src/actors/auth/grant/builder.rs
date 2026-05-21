use url::Url;

use pubky_common::{
    auth::jws::ClientId,
    crypto::{Keypair, random_bytes},
};

use crate::actors::DEFAULT_HTTP_RELAY_INBOX;
use crate::actors::auth::deep_links::{DeepLink, SigninGrantDeepLink, SignupGrantDeepLink};
use crate::actors::auth::grant::flow::PubkyGrantAuthFlow;
use crate::actors::auth::kind::AuthFlowKind;
use crate::actors::auth::relay::auth_relay_listener::AuthRelayListener;
use crate::errors::Result;
use crate::{Capabilities, PubkyHttpClient};

/// Builder for the **Grant + `PoP`** [`PubkyGrantAuthFlow`].
///
/// - The deep link gains `cid=<client_id>` and `cpk=<client_pk_z32>` params.
/// - The signer signs a `pubky-grant` JWS instead of a legacy `AuthToken`.
/// - The resulting [`PubkyGrantAuthFlow`] yields a grant-backed session that
///   self-refreshes.
#[derive(Debug, Clone)]
pub struct GrantAuthFlowBuilder {
    caps: Capabilities,
    base_relay: Url,
    client: Option<PubkyHttpClient>,
    auth_kind: AuthFlowKind,
    client_secret: [u8; 32],
    client_id: ClientId,
    client_keypair: Option<Keypair>,
}

impl GrantAuthFlowBuilder {
    pub(crate) fn new(caps: Capabilities, auth_kind: AuthFlowKind, client_id: ClientId) -> Self {
        Self {
            caps,
            base_relay: Url::parse(DEFAULT_HTTP_RELAY_INBOX)
                .expect("Should be able to parse the default HTTP relay"),
            client: None,
            auth_kind,
            client_secret: random_bytes::<32>(),
            client_id,
            client_keypair: None,
        }
    }

    /// Set a custom relay base URL. Trailing slash optional.
    #[must_use]
    pub fn relay(mut self, url: Url) -> Self {
        self.base_relay = url;
        self
    }

    /// Provide a custom `PubkyHttpClient` (e.g., with custom TLS, roots, or test wiring).
    #[must_use]
    pub fn client(mut self, client: PubkyHttpClient) -> Self {
        self.client = Some(client);
        self
    }

    /// Override the random `client_secret`. By default, a fresh 32-byte secret is generated.
    #[must_use]
    pub fn client_secret(mut self, client_secret: [u8; 32]) -> Self {
        self.client_secret = client_secret;
        self
    }

    /// Pin a specific Ed25519 keypair as the grant's `cnf` claim and `PoP` signer.
    /// If omitted, a fresh random keypair is generated at [`Self::start`].
    #[must_use]
    pub fn client_keypair(mut self, keypair: Keypair) -> Self {
        self.client_keypair = Some(keypair);
        self
    }

    /// Finalize: derive channel, compute the `pubkyauth://` deep link, spawn
    /// the background poller, and return the flow handle.
    ///
    /// # Errors
    /// - Propagates failures from constructing the default [`PubkyHttpClient`]
    ///   or starting the [`AuthRelayListener`].
    pub fn start(self) -> Result<PubkyGrantAuthFlow> {
        let Self {
            caps,
            base_relay,
            client,
            auth_kind,
            client_secret,
            client_id,
            client_keypair,
        } = self;

        let client = match client {
            Some(c) => c,
            None => PubkyHttpClient::new()?,
        };

        let client_keypair = client_keypair.unwrap_or_else(Keypair::random);
        let client_pk = client_keypair.public_key();

        let auth_url = match auth_kind {
            AuthFlowKind::SignIn => DeepLink::SigninGrant(SigninGrantDeepLink::new(
                caps,
                base_relay.clone(),
                client_secret,
                client_id,
                client_pk,
            )),
            AuthFlowKind::SignUp {
                homeserver_public_key,
                signup_token,
            } => {
                let hs_pk = *homeserver_public_key;
                DeepLink::SignupGrant(SignupGrantDeepLink::new(
                    caps,
                    base_relay.clone(),
                    client_secret,
                    hs_pk.clone(),
                    signup_token.clone(),
                    client_id,
                    client_pk,
                ))
            }
        };

        let relay_listener = AuthRelayListener::builder(client_secret)
            .relay_base_url(base_relay)
            .client(client.clone())
            .start()?;

        Ok(PubkyGrantAuthFlow::new(
            relay_listener,
            client,
            auth_url,
            client_keypair,
        ))
    }
}
