#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]
use std::time::Duration;

use anyhow::Result;
use http_relay::HttpRelay;
use pubky::{ClientBuilder, Keypair};
use pubky_common::timestamp::Timestamp;
use pubky_homeserver::Homeserver;
use url::Url;

/// A local test network for Pubky Core development.
pub struct Testnet {
    dht: mainline::Testnet,
    relays: Vec<pkarr_relay::Relay>,
}

impl Testnet {
    /// Run a new testnet.
    pub async fn run() -> Result<Self> {
        let dht = mainline::Testnet::new(3)?;

        let mut testnet = Self {
            dht,
            relays: vec![],
        };

        testnet.run_pkarr_relay().await?;

        Ok(testnet)
    }

    /// Create these components with hardcoded configurations:
    ///
    /// 1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
    /// 3. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
    /// 2. A Homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
    /// 4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
    pub async fn run_with_hardcoded_configurations() -> Result<Self> {
        let dht = mainline::Testnet::new(3)?;

        dht.leak();

        let storage = std::env::temp_dir().join(Timestamp::now().to_string());

        let mut builder = pkarr_relay::Relay::builder();
        builder
            .http_port(15411)
            .storage(storage.clone())
            .disable_rate_limiter()
            .pkarr(|pkarr| {
                pkarr
                    .request_timeout(Duration::from_millis(100))
                    .bootstrap(&dht.bootstrap)
                    .dht(|builder| {
                        if !dht.bootstrap.first().unwrap().contains("6881") {
                            builder.server_mode().port(6881);
                        }

                        builder
                            .bootstrap(&dht.bootstrap)
                            .request_timeout(Duration::from_millis(200))
                    })
            });
        let relay = unsafe { builder.run() }.await?;

        let mut builder = Homeserver::builder();
        builder
            .keypair(Keypair::from_secret_key(&[0; 32]))
            .storage(storage)
            .bootstrap(&dht.bootstrap)
            .relays(&[relay.local_url()])
            .domain("localhost")
            .close_signups()
            .admin_password("admin".to_string());
        unsafe { builder.run().await }?;

        HttpRelay::builder().http_port(15412).run().await?;

        let testnet = Self {
            dht,
            relays: vec![relay],
        };

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

    /// Run a Pubky Homeserver that requires signup tokens
    pub async fn run_homeserver_with_signup_tokens(&self) -> Result<Homeserver> {
        Homeserver::run_test_with_signup_tokens(&self.dht.bootstrap).await
    }

    /// Run an HTTP Relay
    pub async fn run_http_relay(&self) -> Result<HttpRelay> {
        HttpRelay::builder().run().await
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
