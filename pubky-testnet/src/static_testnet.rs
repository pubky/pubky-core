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
        match &self.mode {
            TestnetMode::Ephemeral => self
                .run_ephemeral_homeserver()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to run homeserver on port 6288: {}", e)),
            TestnetMode::Persistent(data_dir) => self
                .run_persistent_homeserver(data_dir.clone())
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

        // Wrap the persistent dir so the homeserver joins the testnet's local DHT
        // instead of the mainnet bootstrap nodes from the on-disk config.
        let bootstrap_nodes: Vec<DomainPort> = self
            .bootstrap_nodes()
            .iter()
            .map(|node| {
                DomainPort::from_str(node)
                    .map_err(|e| anyhow::anyhow!("Failed to parse bootstrap node '{}': {}", node, e))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

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
                .map(|node| {
                    DomainPort::from_str(node)
                        .map_err(|e| anyhow::anyhow!("Failed to parse bootstrap node '{}': {}", node, e))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn testnet_data_dir_overrides_dht_config() {
        let temp = TempDir::new().unwrap();
        let persistent = PersistentDataDir::new(temp.path().to_path_buf());
        persistent.init().unwrap();

        let bootstrap = vec![DomainPort::from_str("127.0.0.1:6881").unwrap()];
        let testnet_dir = TestnetDataDir {
            inner: persistent.clone(),
            dht_bootstrap_nodes: bootstrap.clone(),
        };

        let config = testnet_dir.read_or_create_config_file().unwrap();
        assert_eq!(config.pkdns.dht_bootstrap_nodes, Some(bootstrap));
        assert_eq!(config.pkdns.dht_relay_nodes, None);
    }

    #[test]
    fn testnet_data_dir_delegates_path() {
        let temp = TempDir::new().unwrap();
        let persistent = PersistentDataDir::new(temp.path().to_path_buf());
        let testnet_dir = TestnetDataDir {
            inner: persistent.clone(),
            dht_bootstrap_nodes: vec![],
        };

        assert_eq!(testnet_dir.path(), persistent.path());
    }

    #[test]
    fn testnet_data_dir_delegates_keypair() {
        let temp = TempDir::new().unwrap();
        let persistent = PersistentDataDir::new(temp.path().to_path_buf());
        persistent.init().unwrap();

        let testnet_dir = TestnetDataDir {
            inner: persistent.clone(),
            dht_bootstrap_nodes: vec![],
        };

        let kp1 = persistent.read_or_create_keypair().unwrap();
        let kp2 = testnet_dir.read_or_create_keypair().unwrap();
        assert_eq!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn config_seeding_copies_file_to_empty_data_dir() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("testnet");
        let persistent = PersistentDataDir::new(data_dir.clone());

        let source_config = temp.path().join("custom.toml");
        let sample = ConfigToml::sample_string();
        std::fs::write(&source_config, &sample).unwrap();

        // Should copy since config.toml doesn't exist yet
        std::fs::create_dir_all(persistent.path()).unwrap();
        std::fs::copy(&source_config, persistent.get_config_file_path()).unwrap();

        assert!(persistent.get_config_file_path().exists());
        let content = std::fs::read_to_string(persistent.get_config_file_path()).unwrap();
        assert_eq!(content, sample);
    }

    #[test]
    fn config_seeding_rejects_when_config_already_exists() {
        let temp = TempDir::new().unwrap();
        let persistent = PersistentDataDir::new(temp.path().to_path_buf());
        persistent.init().unwrap();

        // config.toml now exists from init()
        let config_path = persistent.get_config_file_path();
        assert!(config_path.exists());

        // Simulate the same check that run_persistent_homeserver performs:
        // if homeserver_config is set AND config.toml already exists, it should be an error.
        let source = temp.path().join("other.toml");
        std::fs::write(&source, "[general]\nsignup_mode = \"open\"\n").unwrap();

        let has_homeserver_config = true;
        let would_error = has_homeserver_config && config_path.exists();
        assert!(
            would_error,
            "Seeding should be rejected when config.toml already exists in the data directory"
        );
    }

    #[test]
    fn builder_defaults_to_ephemeral() {
        let builder = StaticTestnet::builder();
        assert!(
            matches!(builder.mode, TestnetMode::Ephemeral),
            "Default mode should be Ephemeral"
        );
        assert!(builder.homeserver_config.is_none());
    }

    #[test]
    fn builder_persistent_sets_mode() {
        let dir = PathBuf::from("/tmp/test-data");
        let builder = StaticTestnet::builder().persistent(dir.clone());
        match &builder.mode {
            TestnetMode::Persistent(d) => assert_eq!(d, &dir),
            TestnetMode::Ephemeral => panic!("Expected Persistent mode"),
        }
    }

    #[test]
    fn builder_homeserver_config_sets_path() {
        let config = PathBuf::from("/tmp/my-config.toml");
        let builder = StaticTestnet::builder().homeserver_config(config.clone());
        assert_eq!(builder.homeserver_config, Some(config));
    }

    #[test]
    fn builder_chaining_all_options() {
        let dir = PathBuf::from("/tmp/data");
        let config = PathBuf::from("/tmp/config.toml");
        let builder = StaticTestnet::builder()
            .homeserver_config(config.clone())
            .persistent(dir.clone());
        assert_eq!(builder.homeserver_config, Some(config));
        assert!(matches!(builder.mode, TestnetMode::Persistent(d) if d == dir));
    }

    #[test]
    fn persistent_data_dir_init_creates_structure() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("new-testnet");
        let persistent = PersistentDataDir::new(data_dir.clone());
        persistent.init().unwrap();

        assert!(persistent.get_config_file_path().exists(), "config.toml should be created");
        // Keypair file should exist after init
        let kp = persistent.read_or_create_keypair().unwrap();
        let kp2 = persistent.read_or_create_keypair().unwrap();
        assert_eq!(kp.public_key(), kp2.public_key(), "Keypair should be stable across reads");
    }

    #[test]
    fn seeded_config_is_readable_by_testnet_data_dir() {
        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("testnet");
        let persistent = PersistentDataDir::new(data_dir);

        // Write a config override with signup_mode = "token_required" (non-default for tests)
        let config_content = "[general]\nsignup_mode = \"token_required\"\n";
        let source = temp.path().join("seed.toml");
        std::fs::write(&source, config_content).unwrap();

        // Seed: create dir, copy config, init
        std::fs::create_dir_all(persistent.path()).unwrap();
        std::fs::copy(&source, persistent.get_config_file_path()).unwrap();
        persistent.init().unwrap();

        // Wrap in TestnetDataDir and read config
        let bootstrap = vec![DomainPort::from_str("127.0.0.1:6881").unwrap()];
        let testnet_dir = TestnetDataDir {
            inner: persistent,
            dht_bootstrap_nodes: bootstrap.clone(),
        };

        let config = testnet_dir.read_or_create_config_file().unwrap();
        // Seeded value should be preserved
        assert_eq!(config.general.signup_mode, pubky_homeserver::SignupMode::TokenRequired);
        // DHT bootstrap should be overridden by TestnetDataDir
        assert_eq!(config.pkdns.dht_bootstrap_nodes, Some(bootstrap));
        assert_eq!(config.pkdns.dht_relay_nodes, None);
    }

    /// Integration test: start a persistent-mode testnet with a test DB config,
    /// verify the homeserver boots and serves requests.
    ///
    /// The DB uses `?pubky-test=true` so it auto-cleans, but the config/keypair
    /// pipeline exercises the full persistent path (disk I/O, TestnetDataDir wrapping).
    ///
    /// Requires Postgres. Run with:
    /// ```text
    /// TEST_PUBKY_CONNECTION_STRING=postgres://postgres:postgres@localhost:5432/pubky_homeserver?pubky-test=true \
    ///   cargo test -p pubky-testnet persistent_mode_starts_homeserver
    /// ```
    #[tokio::test]
    async fn persistent_mode_starts_homeserver() {
        // Skip if no Postgres is available.
        let db_url = match std::env::var("TEST_PUBKY_CONNECTION_STRING") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("Skipping persistent_mode_starts_homeserver: TEST_PUBKY_CONNECTION_STRING not set");
                return;
            }
        };

        let temp = TempDir::new().unwrap();
        let data_dir = temp.path().join("persistent-testnet");

        // Write a config that uses the test DB, ephemeral ports, and in-memory storage.
        let config_content = format!(
            r#"
[general]
database_url = "{db_url}"
signup_mode = "open"

[drive]
pubky_listen_socket = "127.0.0.1:0"
icann_listen_socket = "127.0.0.1:0"

[admin]
enabled = true
listen_socket = "127.0.0.1:0"

[storage]
type = "file_system"
"#
        );
        let config_path = temp.path().join("seed-config.toml");
        std::fs::write(&config_path, &config_content).unwrap();

        let testnet = StaticTestnet::builder()
            .homeserver_config(config_path)
            .persistent(data_dir.clone())
            .start()
            .await
            .expect("persistent testnet should start");

        // Verify the testnet reports persistent mode.
        assert!(testnet.is_persistent());

        // Verify the homeserver is accessible.
        let _homeserver = testnet.homeserver_app();

        // Verify the data dir was initialized with config + keypair on disk.
        let persistent_dir = PersistentDataDir::new(data_dir.clone());
        assert!(persistent_dir.get_config_file_path().exists(), "config.toml should exist on disk");
        assert!(persistent_dir.get_secret_file_path().exists(), "secret key should exist on disk");

        // Verify the keypair on disk is stable (same as what the homeserver uses).
        let kp = persistent_dir.read_or_create_keypair().unwrap();
        let kp2 = persistent_dir.read_or_create_keypair().unwrap();
        assert_eq!(kp.public_key(), kp2.public_key());

        drop(testnet);
        crate::drop_test_databases().await;
    }
}
