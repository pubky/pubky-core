use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use ::pkarr::{Keypair, PublicKey};
use anyhow::Result;
use http::HttpServers;
use pkarr::PkarrServer;
use tracing::info;

use crate::{
    config::{Config, DEFAULT_HTTPS_PORT, DEFAULT_HTTP_PORT},
    HomeserverCore,
};

mod http;
mod pkarr;

#[derive(Debug, Default)]
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
/// Homeserver [Core][HomeserverCore] + I/O (http server and pkarr publishing).
pub struct Homeserver {
    http_servers: HttpServers,
    keypair: Keypair,
}

impl Homeserver {
    /// Returns a Homeserver builder.
    pub fn builder() -> HomeserverBuilder {
        HomeserverBuilder::default()
    }

    /// Run a Homeserver with a configuration file path.
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async fn run_with_config_file(config_path: impl AsRef<Path>) -> Result<Self> {
        unsafe { Self::run(Config::load(config_path).await?) }.await
    }

    /// Run a Homeserver with configurations suitable for ephemeral tests.
    pub async fn run_test(bootstrap: &[String]) -> Result<Self> {
        let config = Config::test(bootstrap);

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

        let core = unsafe { HomeserverCore::new(config.core)? };

        let http_servers = HttpServers::run(&keypair, &config.io, &core.router).await?;

        info!(
            "Homeserver listening on http://localhost:{}",
            http_servers.http_address().port()
        );

        info!("Publishing Pkarr packet..");

        let pkarr_server = PkarrServer::new(
            &keypair,
            &config.io,
            http_servers.https_address().port(),
            http_servers.http_address().port(),
        )?;
        pkarr_server.publish_server_packet().await?;

        info!("Homeserver listening on https://{}", keypair.public_key());

        Ok(Self {
            http_servers,
            keypair,
        })
    }

    // === Getters ===

    pub fn public_key(&self) -> PublicKey {
        self.keypair.public_key()
    }

    /// Return the `https://<server public key>` url
    pub fn url(&self) -> url::Url {
        url::Url::parse(&format!("https://{}", self.public_key())).expect("valid url")
    }

    // === Public Methods ===

    /// Send a shutdown signal to all open resources
    pub fn shutdown(&self) {
        self.http_servers.shutdown();
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
