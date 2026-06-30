use crate::pubky::Pubky;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::Testnet;
use http_relay::HttpRelay;
use pubky_homeserver::{
    AppContext, ConfigToml, DataDir, DomainPort, HomeserverApp, MockDataDir, PersistentDataDir,
};

/// How the testnet stores homeserver state.
#[derive(Debug, Clone)]
enum TestnetMode {
    /// All state lives in memory / temp dirs and is lost on shutdown.
    Ephemeral,
    /// State is persisted to the given data directory across restarts.
    Persistent(PathBuf),
}

/// Builder for configuring and starting a [`StaticTestnet`].
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> anyhow::Result<()> {
/// use pubky_testnet::StaticTestnet;
///
/// // Ephemeral (default)
/// let testnet = StaticTestnet::builder().start().await?;
///
/// // Ephemeral with custom config
/// let testnet = StaticTestnet::builder()
///     .homeserver_config("my-config.toml".into())
///     .start()
///     .await?;
///
/// // Persistent
/// let testnet = StaticTestnet::builder()
///     .persistent("./my-testnet".into())
///     .start()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct StaticTestnetBuilder {
    homeserver_config: Option<PathBuf>,
    mode: TestnetMode,
}

impl StaticTestnetBuilder {
    fn new() -> Self {
        Self {
            homeserver_config: None,
            mode: TestnetMode::Ephemeral,
        }
    }

    /// Set a custom homeserver config file.
    ///
    /// In ephemeral mode, this overrides the default config.
    /// In persistent mode, this seeds the initial `config.toml` on first run
    /// (errors if one already exists in the data directory).
    pub fn homeserver_config(mut self, path: PathBuf) -> Self {
        self.homeserver_config = Some(path);
        self
    }

    /// Enable persistent mode with the given data directory.
    ///
    /// The directory is auto-initialized on first run (config.toml, secret, data/files/).
    /// On subsequent runs, the existing state is picked up.
    pub fn persistent(mut self, data_dir: PathBuf) -> Self {
        self.mode = TestnetMode::Persistent(data_dir);
        self
    }

    /// Start the testnet with the configured options.
    pub async fn start(self) -> anyhow::Result<StaticTestnet> {
        let mut testnet = StaticTestnet::start_infra(self.homeserver_config, self.mode).await?;
        testnet.run_homeserver().await?;
        Ok(testnet)
    }
}

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
    homeserver_config: Option<PathBuf>,
    /// How the homeserver stores state.
    mode: TestnetMode,
    #[allow(dead_code)]
    fixed_bootstrap_node: Option<pkarr::mainline::Dht>, // Keep alive
    #[allow(dead_code)]
    temp_dirs: Vec<tempfile::TempDir>, // Keep temp dirs alive for the pkarr relay
}

impl StaticTestnet {
    /// Create a builder for configuring the testnet.
    pub fn builder() -> StaticTestnetBuilder {
        StaticTestnetBuilder::new()
    }

    /// Run an ephemeral testnet with the default homeserver config.
    pub async fn start() -> anyhow::Result<Self> {
        Self::builder().start().await
    }

    /// Whether this testnet is running in persistent mode.
    pub fn is_persistent(&self) -> bool {
        matches!(self.mode, TestnetMode::Persistent(_))
    }

    /// Start the shared infrastructure (DHT bootstrap node, pkarr relay, http relay)
    /// without a homeserver.
    async fn start_infra(
        homeserver_config: Option<PathBuf>,
        mode: TestnetMode,
    ) -> anyhow::Result<Self> {
        let testnet = Testnet::new().await?;
        let fixed_boostrap = Self::run_fixed_boostrap_node(&testnet.dht.bootstrap)
            .map_err(|e| anyhow::anyhow!("Failed to run bootstrap node on port 6881: {}", e))?;

        let mut testnet = Self {
            testnet,
            fixed_bootstrap_node: fixed_boostrap,
            temp_dirs: vec![],
            homeserver_config,
            mode,
        };

        testnet
            .run_fixed_pkarr_relays()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run pkarr relay on port 15411: {}", e))?;
        testnet
            .run_fixed_http_relay()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to run http relay on port 15412: {}", e))?;

        Ok(testnet)
    }

    /// Start the homeserver based on the configured mode.
    async fn run_homeserver(&mut self) -> anyhow::Result<()> {
        match self.mode.clone() {
            TestnetMode::Ephemeral => self
                .run_ephemeral_homeserver()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to run homeserver on port 6288: {}", e)),
            TestnetMode::Persistent(data_dir) => self
                .run_persistent_homeserver(data_dir)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to run persistent homeserver: {}", e)),
        }
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
            .cors_allow_all(true)
            .run()
            .await?;
        self.testnet.http_relays.push(relay);
        Ok(())
    }

    async fn run_persistent_homeserver(&mut self, data_dir: PathBuf) -> anyhow::Result<()> {
        let persistent_dir = PersistentDataDir::new(data_dir);
        let config_path = persistent_dir.get_config_file_path();

        if let Some(source) = &self.homeserver_config {
            if config_path.exists() {
                anyhow::bail!(
                    "config.toml already exists at {}. Remove --homeserver-config to use the existing config, \
                     or delete the file to replace it.",
                    config_path.display()
                );
            }
            std::fs::create_dir_all(persistent_dir.path())?;
            std::fs::copy(source, &config_path)?;
            tracing::info!("Copied {} → {}", source.display(), config_path.display());
        }

        persistent_dir.init()?;

        let bootstrap_nodes: Vec<DomainPort> = self
            .bootstrap_nodes()
            .iter()
            .map(|node| DomainPort::from_str(node).unwrap())
            .collect();

        let testnet_dir = TestnetDataDir {
            inner: persistent_dir,
            dht_bootstrap_nodes: bootstrap_nodes,
        };

        let context = AppContext::read_from(testnet_dir).await?;
        let homeserver = HomeserverApp::start(context).await?;
        self.testnet.homeservers.push(homeserver);
        Ok(())
    }

    async fn run_ephemeral_homeserver(&mut self) -> anyhow::Result<()> {
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

/// A [`PersistentDataDir`] wrapper that overrides DHT config for testnet use.
///
/// This ensures the homeserver connects to the testnet's local DHT bootstrap
/// nodes rather than mainnet, while still using persistent on-disk storage.
#[derive(Debug, Clone)]
struct TestnetDataDir {
    inner: PersistentDataDir,
    dht_bootstrap_nodes: Vec<DomainPort>,
}

impl DataDir for TestnetDataDir {
    fn path(&self) -> &Path {
        self.inner.path()
    }

    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        self.inner.ensure_data_dir_exists_and_is_writable()
    }

    fn read_or_create_config_file(&self) -> anyhow::Result<ConfigToml> {
        let mut config = self.inner.read_or_create_config_file()?;
        config.pkdns.dht_bootstrap_nodes = Some(self.dht_bootstrap_nodes.clone());
        config.pkdns.dht_relay_nodes = None;
        Ok(config)
    }

    fn read_or_create_keypair(&self) -> anyhow::Result<pubky_common::crypto::Keypair> {
        self.inner.read_or_create_keypair()
    }
}
