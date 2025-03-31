#![doc = include_str!("../README.md")]
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]
use std::{str::FromStr, time::Duration};

use anyhow::Result;
use http_relay::HttpRelay;
use pubky::Keypair;
use pubky_common::timestamp::Timestamp;
use pubky_homeserver::{ConfigToml, DataDirMock, DomainPort, HomeserverSuite, SignupMode};
use url::Url;


/// A local test network for Pubky Core development.
/// Can create a flexible amount of pkarr relays, http relays and homeservers.
/// 
/// Keeps track of the components and can create new ones.
/// Cleans up all resources when dropped.
pub struct FlexibleTestnet {
    pub(crate) dht: mainline::Testnet,
    pub(crate) pkarr_relays: Vec<pkarr_relay::Relay>,
    pub(crate) http_relays: Vec<HttpRelay>,
    pub(crate) homeservers: Vec<HomeserverSuite>,
}

impl FlexibleTestnet {
    /// Run a new testnet with a local DHT.
    pub async fn new() -> Result<Self> {
        let dht = mainline::Testnet::new(3)?;
        let testnet = Self {
            dht,
            pkarr_relays: vec![],
            http_relays: vec![],
            homeservers: vec![],
        };

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
        let homeserver = HomeserverSuite::run_with_data_dir_mock(mock_dir).await?;

        let http_relay = HttpRelay::builder().http_port(15412).run().await?;

        let testnet = Self {
            dht,
            pkarr_relays: vec![relay],
            http_relays: vec![http_relay],
            homeservers: vec![homeserver],
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
    pub fn dht_bootstrap_nodes(&self) -> Vec<DomainPort> {
        self.dht.bootstrap.iter()
        .map(|s| {
            DomainPort::from_str(s).expect("boostrap nodes from the pkarr dht are always valid domain:port pairs")
        })
        .collect()
    }

    /// Returns a list of pkarr relays.
    pub fn dht_relay_urls(&self) -> Box<[Url]> {
        self.pkarr_relays.iter().map(|r| r.local_url()).collect()
    }

    /// Run the full homeserver suite with core and admin server
    /// Automatically listens on the default ports.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_suite(&mut self) -> Result<&HomeserverSuite> {
        let mock_dir = DataDirMock::new(ConfigToml::test(), Some(Keypair::from_secret_key(&[0; 32])))?;
        self.run_homeserver_suite_with_config(mock_dir).await
    }

    /// Run the full homeserver suite with core and admin server
    /// Automatically listens on the configured ports.
    /// Automatically uses the configured bootstrap nodes and relays in this Testnet.
    pub async fn run_homeserver_suite_with_config(
        &mut self,
        mut mock_dir: DataDirMock,
    ) -> Result<&HomeserverSuite> {
        mock_dir.config_toml.pkdns.dht_bootstrap_nodes = Some(self.dht_bootstrap_nodes());
        if !self.dht_relay_urls().is_empty() {
            mock_dir.config_toml.pkdns.dht_relay_nodes = Some(self.dht_relay_urls().to_vec());
        }
        let homeserver = HomeserverSuite::run_with_data_dir_mock(mock_dir).await?;
        self.homeservers.push(homeserver);
        Ok(self.homeservers.last().expect("homeservers should be non-empty"))
    }

    /// Run an HTTP Relay
    pub async fn run_http_relay(&mut self) -> Result<&HttpRelay> {
        let relay = HttpRelay::builder()
        .http_port(0) // Random available port
        .run().await?;
        self.http_relays.push(relay);
        Ok(self.http_relays.last().expect("http relays should be non-empty"))
    }

    /// Create a [ClientBuilder] and configure it to use this local test network.
    pub fn pubky_client_builder(&self) -> pubky::ClientBuilder {
        let relays = self.dht_relay_urls();

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

        self.pkarr_relays.push(relay);

        Ok(url)
    }
}

mod test {
    use super::*;

    #[tokio::test]
    async fn test_keep_relays_alive_even_when_dropped() {
        let mut testnet = FlexibleTestnet::new().await.unwrap();
        {
            let _relay = testnet.run_http_relay().await.unwrap();
        }
        assert_eq!(testnet.http_relays.len(), 1);
    }
}