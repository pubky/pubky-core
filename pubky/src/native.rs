use std::time::Duration;

use ::pkarr::{
    mainline::dht::{DhtSettings, Testnet},
    PkarrClient, PublicKey, Settings, SignedPacket,
};
use bytes::Bytes;
use pkarr::Keypair;
use pubky_common::session::Session;
use reqwest::{RequestBuilder, Response};
use url::Url;

use crate::{
    error::Result,
    shared::{
        list_builder::ListBuilder,
        recovery_file::{create_recovery_file, decrypt_recovery_file},
    },
    PubkyClient,
};

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

impl Default for PubkyClient {
    fn default() -> Self {
        Self::new()
    }
}

// === Public API ===

impl PubkyClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .cookie_store(true)
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            #[cfg(not(target_arch = "wasm32"))]
            pkarr: PkarrClient::new(Default::default()).unwrap().as_async(),
        }
    }

    pub fn test(testnet: &Testnet) -> Self {
        Self {
            http: reqwest::Client::builder()
                .cookie_store(true)
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            pkarr: PkarrClient::new(Settings {
                dht: DhtSettings {
                    request_timeout: Some(Duration::from_millis(100)),
                    bootstrap: Some(testnet.bootstrap.to_owned()),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })
            .unwrap()
            .as_async(),
        }
    }

    // === Auth ===

    /// Signup to a homeserver and update Pkarr accordingly.
    ///
    /// The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
    /// for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
    pub async fn signup(&self, keypair: &Keypair, homeserver: &PublicKey) -> Result<()> {
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
    pub async fn signin(&self, keypair: &Keypair) -> Result<()> {
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
        create_recovery_file(keypair, passphrase)
    }

    /// Recover a keypair from a recovery file by decrypting the secret key using `passphrase`.
    pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair> {
        decrypt_recovery_file(recovery_file, passphrase)
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

    pub(crate) fn store_session(&self, _: Response) {}
    pub(crate) fn remove_session(&self, _: &PublicKey) {}
}
