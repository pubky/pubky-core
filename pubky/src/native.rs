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

    /// Creates a [PubkyClient] with:
    /// - DHT bootstrap nodes set to the `testnet` bootstrap nodes.
    /// - DHT request timout set to 500 milliseconds. (unless in CI, then it is left as default 2000)
    ///
    /// For more control, you can use [PubkyClient::builder] testnet option.
    pub fn test(testnet: &Testnet) -> PubkyClient {
        let mut builder = PubkyClient::builder().testnet(testnet);

        if std::env::var("CI").is_err() {
            builder = builder.dht_request_timeout(Duration::from_millis(500));
        }

        builder.build()
    }
}
