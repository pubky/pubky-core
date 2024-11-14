use std::time::Duration;

use pkarr::mainline::Testnet;

use crate::PubkyClient;

mod api;
mod internals;

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

    /// Build [PubkyClient]
    pub fn build(self) -> PubkyClient {
        // TODO: convert to Result<PubkyClient>

        let pkarr = pkarr::Client::new(self.pkarr_settings).unwrap();

        PubkyClient {
            http: reqwest::Client::builder()
                .cookie_store(true)
                // .dns_resolver(Arc::new(dns_resolver))
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            pkarr,
        }
    }
}

impl Default for PubkyClient {
    fn default() -> Self {
        PubkyClient::builder().build()
    }
}

impl PubkyClient {
    /// Returns a builder to edit settings before creating [PubkyClient].
    pub fn builder() -> Settings {
        Settings::default()
    }

    /// Create a client connected to the local network
    /// with the bootstrapping node: `localhost:6881`
    pub fn testnet() -> Self {
        Self::test(&Testnet {
            bootstrap: vec!["localhost:6881".to_string()],
            nodes: vec![],
        })
    }

    /// Alias to `PubkyClient::builder().testnet(testnet).build()`
    pub fn test(testnet: &Testnet) -> PubkyClient {
        PubkyClient::builder().testnet(testnet).build()
    }
}
