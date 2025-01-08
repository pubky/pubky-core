use std::path::PathBuf;

use ::pkarr::{Keypair, PublicKey};
use anyhow::Result;
use http::HttpServers;
use pkarr::PkarrServer;
use tracing::info;

use crate::{Config, HomeserverCore};

mod http;
mod pkarr;

#[derive(Debug, Default)]
pub struct HomeserverBuilder(Config);

impl HomeserverBuilder {
    pub fn testnet(mut self) -> Self {
        self.0.testnet = true;

        self
    }

    /// Configure the Homeserver's keypair
    pub fn keypair(mut self, keypair: Keypair) -> Self {
        self.0.keypair = keypair;

        self
    }

    /// Configure the Mainline DHT bootstrap nodes. Useful for testnet configurations.
    pub fn bootstrap(mut self, bootstrap: Vec<String>) -> Self {
        self.0.bootstrap = Some(bootstrap);

        self
    }

    /// Configure the storage path of the Homeserver
    pub fn storage(mut self, storage: PathBuf) -> Self {
        self.0.storage = storage;

        self
    }

    /// Start running a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async unsafe fn start(self) -> Result<Homeserver> {
        Homeserver::start(self.0).await
    }
}

#[derive(Debug)]
/// Homeserver [Core][HomeserverCore] + I/O (http server and pkarr publishing).
pub struct Homeserver {
    http_servers: HttpServers,
    core: HomeserverCore,
}

impl Homeserver {
    pub fn builder() -> HomeserverBuilder {
        HomeserverBuilder::default()
    }

    /// Start running a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub async unsafe fn start(config: Config) -> Result<Self> {
        tracing::debug!(?config, "Starting homeserver with configurations");

        let core = unsafe { HomeserverCore::new(&config)? };

        let http_servers = HttpServers::start(&core).await?;

        info!(
            "Homeserver listening on http://localhost:{}",
            http_servers.http_address().port()
        );

        info!("Publishing Pkarr packet..");

        let pkarr_server = PkarrServer::new(
            &config,
            http_servers.https_address().port(),
            http_servers.http_address().port(),
        )?;
        pkarr_server.publish_server_packet().await?;

        info!("Homeserver listening on https://{}", core.public_key());

        Ok(Self { http_servers, core })
    }

    /// Start a homeserver in a Testnet mode.
    ///
    /// - Homeserver address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
    /// - Run a pkarr Relay on port `15411`
    /// - Use a temporary storage for the both homeserver and relay
    /// - Only publish http port (ignore https port or domain configurations)
    /// - Run an HTTP relay on port `15412`
    ///
    /// # Safety
    /// See [Self::start]
    pub async unsafe fn start_testnet() -> Result<Self> {
        let testnet = mainline::Testnet::new(10)?;

        let storage =
            std::env::temp_dir().join(pubky_common::timestamp::Timestamp::now().to_string());

        let pkarr_relay = unsafe {
            let mut config = pkarr_relay::Config {
                http_port: 15411,
                cache_path: Some(storage.join("pkarr-relay")),
                rate_limiter: None,
                ..Default::default()
            };

            config.pkarr_config.dht_config.bootstrap = testnet.bootstrap.clone();
            config.pkarr_config.resolvers = Some(vec![].into());

            pkarr_relay::Relay::start(config).await?
        };

        let http_relay = http_relay::HttpRelay::builder()
            .http_port(15412)
            .build()
            .await?;

        tracing::info!(http_relay=?http_relay.local_link_url().as_str(), "Running http relay in Testnet mode");
        tracing::info!(relay_address=?pkarr_relay.relay_address(), bootstrap=?pkarr_relay.resolver_address(),"Running pkarr relay in Testnet mode");

        unsafe {
            Homeserver::builder()
                .testnet()
                .keypair(Keypair::from_secret_key(&[0; 32]))
                .bootstrap(testnet.bootstrap)
                .storage(storage.join("pubky-homeserver"))
                .start()
                .await
        }
    }

    /// Unit tests version of [Homeserver::start], using mainline Testnet, and a temporary storage.
    pub async fn start_test(testnet: &mainline::Testnet) -> Result<Self> {
        unsafe { Homeserver::start(Config::test(testnet)).await }
    }

    // === Getters ===

    pub fn public_key(&self) -> PublicKey {
        self.core.public_key()
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
