use std::{sync::Arc, time::Duration};

use pkarr::mainline::Testnet;

use crate::Client;

mod api;
mod cookies;
mod http;
mod internal;

pub(crate) use cookies::CookieJar;

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Default)]
pub struct Settings {
    pkarr_settings: pkarr::Settings,
}

impl Settings {
    /// Set Pkarr client [pkarr::Settings].
    pub fn pkarr_settings(mut self, settings: pkarr::Settings) -> Self {
        self.pkarr_settings = settings;
        self
    }

    /// Sets the following:
    /// - Pkarr client's DHT bootstrap nodes = `testnet` bootstrap nodes.
    /// - Pkarr client's resolvers           = `testnet` bootstrap nodes.
    /// - Pkarr client's DHT request timout  = 500 milliseconds. (unless in CI, then it is left as default 2000)
    pub fn testnet(mut self, testnet: &Testnet) -> Self {
        let bootstrap = testnet.bootstrap.clone();

        let mut dht_settings = pkarr::mainline::Settings::default().bootstrap(&bootstrap);

        if std::env::var("CI").is_err() {
            dht_settings = dht_settings.request_timeout(Duration::from_millis(500));
        }

        self.pkarr_settings = self
            .pkarr_settings
            .dht_settings(dht_settings)
            .resolvers(Some(bootstrap));

        self
    }

    /// Build [Client]
    pub fn build(self) -> Result<Client, std::io::Error> {
        let pkarr = pkarr::Client::new(self.pkarr_settings)?;

        let cookie_store = Arc::new(CookieJar::default());

        // TODO: allow custom user agent, but force a Pubky user agent information
        let user_agent = DEFAULT_USER_AGENT;

        let http = reqwest::ClientBuilder::from(pkarr.clone())
            // TODO: use persistent cookie jar
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent)
            .build()
            .expect("config expected to not error");

        let icann_http = reqwest::ClientBuilder::new()
            .cookie_provider(cookie_store.clone())
            .user_agent(user_agent)
            .build()
            .expect("config expected to not error");

        Ok(Client {
            cookie_store,
            http,
            icann_http,
            pkarr,
        })
    }
}

impl Client {
    /// Create a new [Client] with default [Settings]
    pub fn new() -> Result<Self, std::io::Error> {
        Self::builder().build()
    }

    /// Returns a builder to edit settings before creating [Client].
    pub fn builder() -> Settings {
        Settings::default()
    }

    /// Create a client connected to the local network
    /// with the bootstrapping node: `localhost:6881`
    pub fn testnet() -> Result<Self, std::io::Error> {
        Self::builder()
            .testnet(&Testnet {
                bootstrap: vec!["localhost:6881".to_string()],
                nodes: vec![],
            })
            .build()
    }

    #[cfg(test)]
    /// Alias to `pubky::Client::builder().testnet(testnet).build().unwrap()`
    pub(crate) fn test(testnet: &Testnet) -> Client {
        Client::builder().testnet(testnet).build().unwrap()
    }
}
