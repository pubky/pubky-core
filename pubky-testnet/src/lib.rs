use anyhow::Result;
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

    // pub fn pubky_client() -> std::result::Result<pubky::Client, pubky::errors::BuildError> {}

    /// Run a new Pkarr relay.
    ///
    /// You can access the list of relays at [Self::relays].
    pub async fn run_pkarr_relay(&mut self) -> Result<Url> {
        let relay = pkarr_relay::Relay::run_test(&self.dht).await?;

        Ok(relay.local_url())
    }
}
