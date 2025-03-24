use std::{net::SocketAddr, path::PathBuf, time::Duration};

use super::http::HttpServers;
use super::key_republisher::HomeserverKeyRepublisher;
use crate::{data_directory::DataDir, SignupMode};
use anyhow::Result;
use pkarr::{Keypair, PublicKey};
use tracing::info;

use crate::core::{AdminConfig, CoreConfig, HomeserverCore};

pub const DEFAULT_HTTP_PORT: u16 = 6286;
pub const DEFAULT_HTTPS_PORT: u16 = 6287;

#[derive(Debug, Default)]
/// Builder for [Homeserver].
pub struct HomeserverBuilder(Config);

impl HomeserverBuilder {
    /// Set the Homeserver's keypair
    pub fn keypair(&mut self, keypair: Keypair) -> &mut Self {
        self.0.keypair = keypair;

        self
    }

    /// Configure the storage path of the Homeserver
    pub fn storage(&mut self, storage: PathBuf) -> &mut Self {
        self.0.core.storage = storage;

        self
    }

    /// Configure the DHT bootstrapping nodes that this Homeserver is connected to.
    pub fn bootstrap(&mut self, bootstrap: &[String]) -> &mut Self {
        self.0.io.bootstrap = Some(bootstrap.to_vec());

        self
    }

    /// Configure Pkarr relays used by this Homeserver
    pub fn relays(&mut self, _relays: &[url::Url]) -> &mut Self {
        // TODO: make it not a noop if we are going to support relays in homeservers.

        self
    }

    /// Set the public domain of this Homeserver
    pub fn domain(&mut self, domain: &str) -> &mut Self {
        self.0.io.domain = Some(domain.to_string());

        self
    }

    /// Set the signup mode to "token_required". Only to be used on ::test()
    /// homeserver for the specific case of testing signup token flow.
    pub fn close_signups(&mut self) -> &mut Self {
        self.0.admin.signup_mode = SignupMode::TokenRequired;

        self
    }

    /// Set a password to protect admin endpoints
    pub fn admin_password(&mut self, password: String) -> &mut Self {
        self.0.admin.password = Some(password);

        self
    }

    /// Run a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async unsafe fn run(self) -> Result<Homeserver> {
        Homeserver::run(self.0).await
    }
}

#[derive(Debug)]
/// Homeserver Core + I/O (http server and pkarr publishing).
pub struct Homeserver {
    http_servers: HttpServers,
    keypair: Keypair,
    key_republisher: HomeserverKeyRepublisher,
}

impl Homeserver {
    /// Returns a Homeserver builder.
    pub fn builder() -> HomeserverBuilder {
        HomeserverBuilder::default()
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn run_with_data_dir(dir_path: PathBuf) -> Result<Self> {
        let data_dir = DataDir::new(dir_path);
        let config = Config::try_from(data_dir)?;
        unsafe { Self::run(config) }.await
    }

    /// Run a Homeserver with configurations suitable for ephemeral tests.
    pub async fn run_test(bootstrap: &[String]) -> Result<Self> {
        let config = Config::test(bootstrap);

        unsafe { Self::run(config) }.await
    }

    /// Run a Homeserver with configurations suitable for ephemeral tests.
    /// That requires signup tokens.
    pub async fn run_test_with_signup_tokens(bootstrap: &[String]) -> Result<Self> {
        let mut config = Config::test(bootstrap);
        config.admin.signup_mode = SignupMode::TokenRequired;

        unsafe { Self::run(config) }.await
    }

    /// Run a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    async unsafe fn run(config: Config) -> Result<Self> {
        tracing::debug!(?config, "Running homeserver with configurations");

        let keypair = config.keypair;

        let core = unsafe { HomeserverCore::new(config.core, config.admin)? };

        let http_servers = HttpServers::run(&keypair, &config.io, &core.router).await?;

        let dht_republisher = HomeserverKeyRepublisher::new(
            &keypair,
            &config.io,
            http_servers.https_address().port(),
            http_servers.http_address().port(),
        )?;
        dht_republisher.start_periodic_republish().await?;
        info!(
            "Homeserver listening on http://localhost:{}",
            http_servers.http_address().port()
        );
        info!("Homeserver listening on https://{}", keypair.public_key());

        Ok(Self {
            http_servers,
            keypair,
            key_republisher: dht_republisher,
        })
    }

    // === Getters ===

    /// Returns the public_key of this server.
    pub fn public_key(&self) -> PublicKey {
        self.keypair.public_key()
    }

    /// Returns the `https://<server public key>` url
    pub fn url(&self) -> url::Url {
        url::Url::parse(&format!("https://{}", self.public_key())).expect("valid url")
    }

    // === Public Methods ===

    /// Send a shutdown signal to all open resources
    pub async fn shutdown(&self) {
        self.http_servers.shutdown();
        self.key_republisher.stop_periodic_republish().await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoConfig {
    pub http_port: u16,
    pub https_port: u16,
    pub public_addr: Option<SocketAddr>,
    pub domain: Option<String>,

    /// Bootstrapping DHT nodes.
    ///
    /// Helpful to run the server locally or in testnet.
    pub bootstrap: Option<Vec<String>>,
    pub dht_request_timeout: Option<Duration>,
}

impl Default for IoConfig {
    fn default() -> Self {
        IoConfig {
            https_port: DEFAULT_HTTPS_PORT,
            http_port: DEFAULT_HTTP_PORT,
            public_addr: None,
            domain: None,
            bootstrap: None,
            dht_request_timeout: None,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Server keypair.
    ///
    /// Defaults to a random keypair.
    pub keypair: Keypair,
    pub io: IoConfig,
    pub core: CoreConfig,
    pub admin: AdminConfig,
}

impl Config {
    /// Create test configurations
    pub fn test(bootstrap: &[String]) -> Self {
        let bootstrap = Some(bootstrap.to_vec());

        Self {
            io: IoConfig {
                bootstrap,
                http_port: 0,
                https_port: 0,
                ..Default::default()
            },
            core: CoreConfig::test(),
            admin: AdminConfig::test(),
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            keypair: Keypair::random(),
            io: IoConfig::default(),
            core: CoreConfig::default(),
            admin: AdminConfig::default(),
        }
    }
}

impl TryFrom<DataDir> for Config {
    type Error = anyhow::Error;

    fn try_from(dir: DataDir) -> Result<Self, Self::Error> {
        dir.ensure_data_dir_exists_and_is_writable()?;
        let conf = dir.read_or_create_config_file()?;
        let keypair = dir.read_or_create_keypair()?;

        // TODO: Needs refactoring of the Homeserver Config struct. I am not doing
        // it yet because I am concentrating on the config currently.
        let io = IoConfig {
            http_port: conf.drive.icann_listen_socket.port(),
            https_port: conf.drive.pubky_listen_socket.port(),
            domain: conf.drive.icann_domain,
            public_addr: Some(conf.pkdns.public_socket),
            ..Default::default()
        };

        let core = CoreConfig {
            storage: dir.path().join("data/lmdb"),
            user_keys_republisher_interval: Some(Duration::from_secs(
                conf.pkdns.user_keys_republisher_interval.into(),
            )),
            lmdb_backup_interval: if conf.general.lmdb_backup_interval_s == 0 {
                None
            } else {
                Some(Duration::from_secs(conf.general.lmdb_backup_interval_s))
            },
            ..Default::default()
        };

        let admin = AdminConfig {
            signup_mode: conf.general.signup_mode,
            password: Some(conf.admin.admin_password),
        };

        Ok(Config {
            keypair,
            io,
            core,
            admin,
        })
    }
}
