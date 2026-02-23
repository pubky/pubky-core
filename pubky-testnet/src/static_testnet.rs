use crate::pubky::Pubky;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    str::FromStr,
};

use crate::Testnet;
use http_relay::HttpRelay;
use pubky_homeserver::{ConfigToml, DomainPort, HomeserverApp, MockDataDir};

/// A simple testnet with
///
/// - A local DHT with a boostrap node on port 6881.
/// - pkarr relay on port 15411.
/// - http relay on port 15412.
/// - A homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
/// - An admin server for the homeserver on port 6288.
pub struct StaticTestnet {
    /// Inner flexible testnet.
    pub testnet: Testnet,
    /// Optional path to the homeserver config file if set.
    pub homeserver_config: Option<PathBuf>,
    #[allow(dead_code)]
    fixed_bootstrap_node: Option<pkarr::mainline::Dht>, // Keep alive
    #[allow(dead_code)]
    temp_dirs: Vec<tempfile::TempDir>, // Keep temp dirs alive for the pkarr relay
}

impl StaticTestnet {
    /// Run a new static testnet with the default homeserver config.
    pub async fn start() -> anyhow::Result<Self> {
        Self::new(None).await
    }

    /// Run a new static testnet with a custom homeserver config.
    pub async fn start_with_homeserver_config(config_path: PathBuf) -> anyhow::Result<Self> {
        Self::new(Some(config_path)).await
    }

    /// Run a new simple testnet.
    pub async fn new(config_path: Option<PathBuf>) -> anyhow::Result<Self> {
        let testnet = Testnet::new().await?;
        let fixed_boostrap = Self::run_fixed_boostrap_node(&testnet.dht.bootstrap)
            .map_err(|e| anyhow::anyhow!("Failed to run bootstrap node on port 6881: {}", e))?;

        let mut testnet = Self {
            testnet,
            fixed_bootstrap_node: fixed_boostrap,
            temp_dirs: vec![],
            homeserver_config: config_path,
        };

        testnet
            .run_fixed_pkarr_relays()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run pkarr relay on port 15411: {}", e))?;
        testnet
            .run_fixed_http_relay()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run http relay on port 15412: {}", e))?;
        testnet
            .run_fixed_homeserver()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run homeserver on port 6288: {}", e))?;

        Ok(testnet)
    }

    /// Create an additional homeserver with a random keypair
    pub async fn create_random_homeserver(
        &mut self,
    ) -> anyhow::Result<&pubky_homeserver::HomeserverApp> {
        self.testnet.create_random_homeserver().await
    }

    /// Create a new pubky client builder.
    pub fn client_builder(&self) -> pubky::PubkyHttpClientBuilder {
        self.testnet.client_builder()
    }

    /// Creates a [`pubky::PubkyHttpClient`] pre-configured to use this test network.
    pub fn client(&self) -> Result<pubky::PubkyHttpClient, pubky::BuildError> {
        self.testnet.client()
    }

    /// Creates a [`pubky::Pubky`] SDK facade pre-configured to use this test network.
    ///
    /// This is a convenience method that builds a client from `Self::client_builder`.
    pub fn sdk(&self) -> Result<Pubky, pubky::BuildError> {
        self.testnet.sdk()
    }

    /// Create a new pkarr client builder.
    pub fn pkarr_client_builder(&self) -> pkarr::ClientBuilder {
        self.testnet.pkarr_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver_app(&self) -> &pubky_homeserver::HomeserverApp {
        self.testnet
            .homeservers
            .first()
            .expect("homeservers should be non-empty")
    }

    /// Get the http relay in the testnet.
    pub fn http_relay(&self) -> &HttpRelay {
        self.testnet
            .http_relays
            .first()
            .expect("http relays should be non-empty")
    }

    /// Get the pkarr relay in the testnet.
    pub fn pkarr_relay(&self) -> &pkarr_relay::Relay {
        self.testnet
            .pkarr_relays
            .first()
            .expect("pkarr relays should be non-empty")
    }

    /// Get the bootstrap nodes for the testnet.
    pub fn bootstrap_nodes(&self) -> Vec<String> {
        let mut nodes = vec![];
        if let Some(dht) = &self.fixed_bootstrap_node {
            nodes.push(dht.info().local_addr().to_string());
        }
        nodes.extend(
            self.testnet
                .dht_bootstrap_nodes()
                .iter()
                .map(|node| node.to_string()),
        );
        nodes
    }

    /// Create a fixed bootstrap node on port 6881 if it is not already running.
    /// If it's already running, return None.
    fn run_fixed_boostrap_node(
        other_bootstrap_nodes: &[String],
    ) -> anyhow::Result<Option<pkarr::mainline::Dht>> {
        if other_bootstrap_nodes
            .iter()
            .any(|node| node.contains("6881"))
        {
            return Ok(None);
        }

        let mut builder = pkarr::mainline::Dht::builder();
        let dht = builder
            .port(6881)
            .bootstrap(other_bootstrap_nodes)
            .server_mode()
            .build()?;
        Ok(Some(dht))
    }

    /// Creates a fixed pkarr relay on port 15411 with a temporary storage directory.
    async fn run_fixed_pkarr_relays(&mut self) -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?; // Gets cleaned up automatically when it drops
        let mut builder = pkarr_relay::Relay::builder();
        builder
            .http_port(15411)
            .storage(temp_dir.path().to_path_buf())
            .disable_rate_limiter()
            .pkarr(|pkarr| {
                pkarr.no_default_network();
                pkarr.bootstrap(&self.testnet.dht.bootstrap)
            });
        let relay = unsafe { builder.run() }.await?;
        self.testnet.pkarr_relays.push(relay);
        self.temp_dirs.push(temp_dir);
        Ok(())
    }

    /// Creates a fixed http relay on port 15412.
    async fn run_fixed_http_relay(&mut self) -> anyhow::Result<()> {
        let relay = HttpRelay::builder()
            .http_port(15412) // Random available port
            .run()
            .await?;
        self.testnet.http_relays.push(relay);
        Ok(())
    }

    async fn run_fixed_homeserver(&mut self) -> anyhow::Result<()> {
        let mut config = if let Some(config_path) = &self.homeserver_config {
            ConfigToml::from_file(config_path)?
        } else {
            ConfigToml::default_test_config()
        };
        let keypair = pubky_common::crypto::Keypair::from_secret(&[0; 32]);
        config.pkdns.dht_bootstrap_nodes = Some(
            self.bootstrap_nodes()
                .iter()
                .map(|node| DomainPort::from_str(node).unwrap())
                .collect(),
        );
        config.pkdns.dht_relay_nodes = None;
        config.drive.icann_listen_socket =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 6286);
        config.drive.pubky_listen_socket =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 6287);
        config.admin.enabled = true; // Enable admin server for static testnet
        config.admin.listen_socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 6288);
        let mock = MockDataDir::new(config, Some(keypair))?;

        let homeserver = HomeserverApp::start_with_mock_data_dir(mock).await?;
        self.testnet.homeservers.push(homeserver);
        Ok(())
    }
}
