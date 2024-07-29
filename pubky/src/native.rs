use std::time::Duration;

use ::pkarr::{
    mainline::dht::{DhtSettings, Testnet},
    PkarrClient, PublicKey, Settings, SignedPacket,
};
use bytes::Bytes;
use pkarr::Keypair;
use pubky_common::session::Session;
use reqwest::{Method, RequestBuilder, Response};
use url::Url;

use crate::{error::Result, PubkyClient};

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
                    request_timeout: Some(Duration::from_millis(10)),
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
    /// Returns an [Error::NotSignedIn] if so, or [reqwest::Error] if
    /// the response has any other `>=400` status code.
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
    pub async fn put(&self, pubky: &PublicKey, path: &str, content: &[u8]) -> Result<()> {
        self.inner_put(pubky, path, content).await
    }

    /// Download a small payload from a given path relative to a pubky author.
    pub async fn get(&self, pubky: &PublicKey, path: &str) -> Result<Bytes> {
        self.inner_get(pubky, path).await
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

    pub(crate) fn store_session(&self, response: Response) {}
    pub(crate) fn remove_session(&self, pubky: &PublicKey) {}
}
