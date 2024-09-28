use std::net::ToSocketAddrs;
use std::time::Duration;

use bytes::Bytes;
use pubky_common::{
    capabilities::Capabilities,
    recovery_file::{create_recovery_file, decrypt_recovery_file},
    session::Session,
};
use reqwest::{RequestBuilder, Response};
use tokio::sync::oneshot;
use url::Url;

use pkarr::{Keypair, PkarrClientAsync};

use ::pkarr::{mainline::dht::Testnet, PkarrClient, PublicKey, SignedPacket};

use crate::{
    error::{Error, Result},
    shared::list_builder::ListBuilder,
    PubkyClient,
};

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Default)]
pub struct PubkyClientBuilder {
    pkarr_settings: pkarr::Settings,
}

impl PubkyClientBuilder {
    /// Set Pkarr client [pkarr::Settings].
    pub fn pkarr_settings(mut self, settings: pkarr::Settings) -> Self {
        self.pkarr_settings = settings;
        self
    }

    /// Use the bootstrap nodes of a testnet, as the bootstrap nodes and
    /// resolvers in the internal Pkarr client.
    pub fn testnet(mut self, testnet: &Testnet) -> Self {
        self.pkarr_settings.dht.bootstrap = testnet.bootstrap.to_vec().into();

        self.pkarr_settings.resolvers = testnet
            .bootstrap
            .iter()
            .flat_map(|resolver| resolver.to_socket_addrs())
            .flatten()
            .collect::<Vec<_>>()
            .into();

        self
    }

    /// Set the request_timeout of the UDP socket in the Mainline DHT client in
    /// the internal Pkarr client.
    ///
    /// Useful to speed unit tests.
    /// Defaults to 2 seconds.
    pub fn dht_request_timeout(mut self, timeout: Duration) -> Self {
        self.pkarr_settings.dht.request_timeout = timeout.into();
        self
    }

    /// Build [PubkyClient]
    pub fn build(self) -> PubkyClient {
        PubkyClient {
            http: reqwest::Client::builder()
                .cookie_store(true)
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            pkarr: PkarrClient::new(self.pkarr_settings).unwrap().as_async(),
        }
    }
}

impl Default for PubkyClient {
    fn default() -> Self {
        PubkyClient::builder().build()
    }
}

// === Public API ===

impl PubkyClient {
    /// Returns a builder to edit settings before creating [PubkyClient].
    pub fn builder() -> PubkyClientBuilder {
        PubkyClientBuilder::default()
    }

    /// Create a client connected to the local network
    /// with the bootstrapping node: `localhost:6881`
    pub fn testnet() -> Self {
        Self::test(&Testnet {
            bootstrap: vec!["localhost:6881".to_string()],
            nodes: vec![],
        })
    }

    /// Creates a [PubkyClient] with:
    /// - DHT bootstrap nodes set to the `testnet` bootstrap nodes.
    /// - DHT request timout set to 500 milliseconds. (unless in CI, then it is left as default 2000)
    ///
    /// For more control, you can use [PubkyClientBuilder::testnet]
    pub fn test(testnet: &Testnet) -> PubkyClient {
        let mut builder = PubkyClient::builder().testnet(testnet);

        if std::env::var("CI").is_err() {
            builder = builder.dht_request_timeout(Duration::from_millis(500));
        }

        builder.build()
    }

    // === Getters ===

    /// Returns a reference to the internal [pkarr] Client.
    pub fn pkarr(&self) -> &PkarrClientAsync {
        &self.pkarr
    }

    // === Auth ===

    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<Session> {
        self.inner_signup(keypair, homeserver).await
    }

    /// Check the current sesison for a given Pubky in its homeserver.
    ///
    /// Returns [Session] or `None` (if recieved `404 NOT_FOUND`),
    /// or [reqwest::Error] if the response has any other `>=400` status code.
    pub async fn session(&self, pubky: &PublicKey) -> Result<Option<Session>> {
        self.inner_session(pubky).await
    }

    /// Signout from a homeserver.
    pub async fn signout(&self, pubky: &PublicKey) -> Result<()> {
        self.inner_signout(pubky).await
    }

    /// Signin to a homeserver.
    pub async fn signin(&self, keypair: &Keypair) -> Result<Session> {
        self.inner_signin(keypair).await
    }

    // === Public data ===

    /// Upload a small payload to a given path.
    pub async fn put<T: TryInto<Url>>(&self, url: T, content: &[u8]) -> Result<()> {
        self.inner_put(url, content).await
    }

    /// Download a small payload from a given path relative to a pubky author.
    pub async fn get<T: TryInto<Url>>(&self, url: T) -> Result<Option<Bytes>> {
        self.inner_get(url).await
    }

    /// Delete a file at a path relative to a pubky author.
    pub async fn delete<T: TryInto<Url>>(&self, url: T) -> Result<()> {
        self.inner_delete(url).await
    }

    /// Returns a [ListBuilder] to help pass options before calling [ListBuilder::send].
    ///
    /// `url` sets the path you want to lest within.
    pub fn list<T: TryInto<Url>>(&self, url: T) -> Result<ListBuilder> {
        self.inner_list(url)
    }

    // === Helpers ===

    /// Create a recovery file of the `keypair`, containing the secret key encrypted
    /// using the `passphrase`.
    pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Result<Vec<u8>> {
        Ok(create_recovery_file(keypair, passphrase)?)
    }

    /// Recover a keypair from a recovery file by decrypting the secret key using `passphrase`.
    pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair> {
        Ok(decrypt_recovery_file(recovery_file, passphrase)?)
    }

    /// Return `pubkyauth://` url and wait for the incoming [AuthToken]
    /// verifying that AuthToken, and if capabilities were requested, signing in to
    /// the Pubky's homeserver and returning the [Session] information.
    pub fn auth_request(
        &self,
        relay: impl TryInto<Url>,
        capabilities: &Capabilities,
    ) -> Result<(Url, tokio::sync::oneshot::Receiver<PublicKey>)> {
        let mut relay: Url = relay
            .try_into()
            .map_err(|_| Error::Generic("Invalid relay Url".into()))?;

        let (pubkyauth_url, client_secret) = self.create_auth_request(&mut relay, capabilities)?;

        let (tx, rx) = oneshot::channel::<PublicKey>();

        let this = self.clone();

        tokio::spawn(async move {
            let to_send = this
                .subscribe_to_auth_response(relay, &client_secret)
                .await?;

            tx.send(to_send)
                .map_err(|_| Error::Generic("Failed to send the session after signing in with token, since the receiver is dropped".into()))?;

            Ok::<(), Error>(())
        });

        Ok((pubkyauth_url, rx))
    }

    /// Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
    /// source of the pubkyauth request url.
    pub async fn send_auth_token<T: TryInto<Url>>(
        &self,
        keypair: &Keypair,
        pubkyauth_url: T,
    ) -> Result<()> {
        let url: Url = pubkyauth_url.try_into().map_err(|_| Error::InvalidUrl)?;

        self.inner_send_auth_token(keypair, url).await?;

        Ok(())
    }
}

// === Internals ===

impl PubkyClient {
    // === Pkarr ===

    pub(crate) async fn pkarr_resolve(
        &self,
        public_key: &PublicKey,
    ) -> Result<Option<SignedPacket>> {
        Ok(self.pkarr.resolve(public_key).await?)
    }

    pub(crate) async fn pkarr_publish(&self, signed_packet: &SignedPacket) -> Result<()> {
        Ok(self.pkarr.publish(signed_packet).await?)
    }

    // === HTTP ===

    pub(crate) fn request(&self, method: reqwest::Method, url: Url) -> RequestBuilder {
        self.http.request(method, url)
    }

    pub(crate) fn store_session(&self, _: &Response) {}
    pub(crate) fn remove_session(&self, _: &PublicKey) {}
}
