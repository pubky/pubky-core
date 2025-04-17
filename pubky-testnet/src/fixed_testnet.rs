use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};

use crate::FlexibleTestnet;
use http_relay::HttpRelay;
use pubky_homeserver::{ConfigToml, MockDataDir, DomainPort, HomeserverSuite, SignupMode};

/// A simple testnet with
///
/// - A local DHT with a boostrap node on port 6881.
/// - pkarr relay on port 15411.
/// - http relay on port 15412.
/// - A homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
/// - An admin server for the homeserver on port 6288.
pub struct FixedTestnet {
    /// Inner flexible testnet.
    pub flexible_testnet: FlexibleTestnet,
    #[allow(dead_code)]
    fixed_bootstrap_node: Option<pkarr::mainline::Dht>, // Keep alive
    #[allow(dead_code)]
    temp_dirs: Vec<tempfile::TempDir>, // Keep temp dirs alive for the pkarr relay
}

impl FixedTestnet {
    /// Run a new simple testnet.
    pub async fn start() -> anyhow::Result<Self> {
        let testnet = FlexibleTestnet::new().await?;
        let fixed_boostrap = Self::run_fixed_boostrap_node(&testnet.dht.bootstrap)
            .map_err(|e| anyhow::anyhow!("Failed to run bootstrap node on port 6881: {}", e))?;
        let mut me = Self {
            flexible_testnet: testnet,
            fixed_bootstrap_node: fixed_boostrap,
            temp_dirs: vec![],
        };

        me.run_fixed_pkarr_relays()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run pkarr relay on port 15411: {}", e))?;
        me.run_fixed_http_relay()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run http relay on port 15412: {}", e))?;
        me.run_fixed_homeserver()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run homeserver on port 6288: {}", e))?;

        Ok(me)
    }

    /// Create a new pubky client builder.
    pub fn pubky_client_builder(&self) -> pubky::ClientBuilder {
        self.flexible_testnet.pubky_client_builder()
    }

    pub fn pkarr_client_builder(&self) -> pkarr::ClientBuilder {
        self.flexible_testnet.pkarr_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver_suite(&self) -> &pubky_homeserver::HomeserverSuite {
        self.flexible_testnet
            .homeservers
            .first()
            .expect("homeservers should be non-empty")
    }

    /// Get the http relay in the testnet.
    pub fn http_relay(&self) -> &HttpRelay {
        self.flexible_testnet
            .http_relays
            .first()
            .expect("http relays should be non-empty")
    }

    /// Get the pkarr relay in the testnet.
    pub fn pkarr_relay(&self) -> &pkarr_relay::Relay {
        self.flexible_testnet
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
            self.flexible_testnet
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
        builder
            .port(6881)
            .bootstrap(other_bootstrap_nodes)
            .server_mode();
        let dht = builder.build()?;
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
                pkarr.bootstrap(&self.flexible_testnet.dht.bootstrap)
            });
        let relay = unsafe { builder.run() }.await?;
        self.flexible_testnet.pkarr_relays.push(relay);
        self.temp_dirs.push(temp_dir);
        Ok(())
    }

    /// Creates a fixed http relay on port 15412.
    async fn run_fixed_http_relay(&mut self) -> anyhow::Result<()> {
        let relay = HttpRelay::builder()
            .http_port(15412) // Random available port
            .run()
            .await?;
        self.flexible_testnet.http_relays.push(relay);
        Ok(())
    }

    async fn run_fixed_homeserver(&mut self) -> anyhow::Result<()> {
        let keypair = pkarr::Keypair::from_secret_key(&[0; 32]);
        let mut config = ConfigToml::test();
        config.pkdns.dht_bootstrap_nodes = Some(
            self.bootstrap_nodes()
                .iter()
                .map(|node| DomainPort::from_str(node).unwrap())
                .collect(),
        );
        config.general.signup_mode = SignupMode::Open;
        config.drive.icann_listen_socket =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 6286);
        config.drive.pubky_listen_socket =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 6287);
        config.admin.listen_socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 6288);
        let mock = MockDataDir::new(config, Some(keypair))?;

        let homeserver = HomeserverSuite::start_with_mock_data_dir(mock).await?;
        self.flexible_testnet.homeservers.push(homeserver);
        Ok(())
    }
}
