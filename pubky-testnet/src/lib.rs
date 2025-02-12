use anyhow::Result;
use http_relay::HttpRelay;
use pubky::ClientBuilder;
use pubky_homeserver::Homeserver;
use url::Url;

pub struct Testnet {
    dht: mainline::Testnet,
    relays: Vec<pkarr_relay::Relay>,
}

impl Testnet {
    pub async fn run() -> Result<Self> {
        let dht = mainline::Testnet::new(10)?;

        let mut testnet = Self {
            dht,
            relays: vec![],
        };

        testnet.run_pkarr_relay().await?;

        Ok(testnet)
    }

    // === Getters ===

    /// Returns a list of DHT bootstrapping nodes.
    pub fn bootstrap(&self) -> &[String] {
        &self.dht.bootstrap
    }

    /// Returns a list of pkarr relays.
    pub fn relays(&self) -> Box<[Url]> {
        self.relays.iter().map(|r| r.local_url()).collect()
    }

    // === Public Methods ===

    /// Run a Pubky Homeserver
    pub async fn run_homeserver(&self) -> Result<Homeserver> {
        Homeserver::run_test(&self.dht.bootstrap).await
    }

    /// Run an HTTP Relay
    pub async fn run_http_relay(&self) -> Result<HttpRelay> {
        HttpRelay::builder().build().await
    }

    /// Create a [ClientBuilder] and configure it to use this local test network.
    pub fn client_builder(&self) -> ClientBuilder {
        let bootstrap = self.bootstrap();
        let relays = self.relays();

        let mut builder = pubky::Client::builder();
        builder.pkarr(|builder| {
            builder
                .bootstrap(bootstrap)
                .relays(&relays)
                .expect("testnet relays should be valid urls")
        });

        builder
    }

    /// Run a new Pkarr relay.
    ///
    /// You can access the list of relays at [Self::relays].
    pub async fn run_pkarr_relay(&mut self) -> Result<Url> {
        let relay = pkarr_relay::Relay::run_test(&self.dht).await?;

        let url = relay.local_url();

        self.relays.push(relay);

        Ok(url)
    }
}
