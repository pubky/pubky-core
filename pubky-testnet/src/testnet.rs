#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]
use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::Result;
use http_relay::HttpRelay;
use pubky::{ClientBuilder, Keypair};
use pubky_common::timestamp::Timestamp;
use pubky_homeserver::{ConfigToml, DataDirMock, DomainPort, HomeserverCore, HomeserverSuite, SignupMode};
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

        let mut config = ConfigToml::test();
        config.pkdns.dht_bootstrap_nodes = Some(Self::bootstrap_domain_port(&dht.bootstrap));
        config.general.signup_mode = SignupMode::TokenRequired;
        config.admin.admin_password = "admin".to_string();
        let mock_dir = DataDirMock::new(config, Some(Keypair::from_secret_key(&[0; 32])))?;
        let homeserver = HomeserverSuite::run_with_data_dir_trait(Arc::new(mock_dir)).await?;

        HttpRelay::builder().http_port(15412).run().await?;

        let testnet = Self {
            dht,
            relays: vec![relay],
        };

        Ok(testnet)
    }

    fn bootstrap_domain_port(bootstrap: &[String]) -> Vec<DomainPort> {
        bootstrap
            .iter()
            .map(|s| {
                DomainPort::from_str(s).expect("boostrap nodes are always valid domain:port pairs")
            })
            .collect()
    }

    // === Getters ===

    /// Returns a list of DHT bootstrapping nodes.
    pub fn bootstrap(&self) -> Vec<DomainPort> {
        Self::bootstrap_domain_port(&self.dht.bootstrap)
    }

    /// Returns a list of pkarr relays.
    pub fn relays(&self) -> Box<[Url]> {
        self.relays.iter().map(|r| r.local_url()).collect()
    }

    /// Run a Pubky HomeserverCore.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_core(&self) -> Result<HomeserverCore> {
        self.run_homeserver_core_with_config(ConfigToml::test()).await
    }

    /// Run a Pubky HomeserverCore.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_core_with_config(&self, mut config: ConfigToml) -> Result<HomeserverCore> {
        config.pkdns.dht_bootstrap_nodes = Some(self.bootstrap());
        if !self.relays().is_empty() {
            config.pkdns.dht_relay_nodes = Some(self.relays().to_vec());
        }
        let mock_dir = DataDirMock::new(config, Some(Keypair::from_secret_key(&[0; 32])))?;
        let mut homeserver = HomeserverCore::from_mock_dir(mock_dir).await?;
        homeserver.listen().await?;
        Ok(homeserver)
    }

    /// Run the full homeserver suite with core and admin server
    /// Automatically listens on the default ports.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_suite(&self) -> Result<HomeserverSuite> {
        self.run_homeserver_suite_with_config(ConfigToml::test()).await
    }

    /// Run the full homeserver suite with core and admin server
    /// Automatically listens on the configured ports.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_suite_with_config(
        &self,
        mut config: ConfigToml,
    ) -> Result<HomeserverSuite> {
        config.pkdns.dht_bootstrap_nodes = Some(self.bootstrap());
        if !self.relays().is_empty() {
            config.pkdns.dht_relay_nodes = Some(self.relays().to_vec());
        }
        let mock_dir = DataDirMock::new(config, Some(Keypair::from_secret_key(&[0; 32])))?;
        let homeserver = HomeserverSuite::run_with_data_dir_trait(Arc::new(mock_dir)).await?;
        Ok(homeserver)
    }

    /// Run an HTTP Relay
    pub async fn run_http_relay(&self) -> Result<HttpRelay> {
        HttpRelay::builder().run().await
    }

    /// Create a [ClientBuilder] and configure it to use this local test network.
    pub fn client_builder(&self) -> ClientBuilder {
        let relays = self.relays();

        let mut builder = pubky::Client::builder();
        builder.pkarr(|builder| {
            builder
                .bootstrap(&self.dht.bootstrap)
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
