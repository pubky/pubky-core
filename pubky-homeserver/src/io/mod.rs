use ::pkarr::{mainline::Testnet, PublicKey};
use anyhow::Result;
use http::HttpServers;
use pkarr::PkarrServer;
use tracing::info;

use crate::{Config, HomeserverCore};

mod http;
mod pkarr;

#[derive(Debug)]
/// Homeserver [Core][HomeserverCore] + I/O (http server and pkarr publishing).
pub struct Homeserver {
    http_servers: HttpServers,
    core: HomeserverCore,
}

impl Homeserver {
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which comes with some safety precautions.
    pub async unsafe fn start(config: Config) -> Result<Self> {
        tracing::debug!(?config, "Starting homeserver with configurations");

        let core = unsafe { HomeserverCore::new(&config)? };

        let http_servers = HttpServers::start(&core).await?;

        info!(
            "Homeserver listening on http://localhost:{}",
            http_servers.http_address().await?.port()
        );

        info!("Publishing Pkarr packet..");

        let pkarr_server = PkarrServer::new(config)?;
        pkarr_server
            .publish_server_packet(http_servers.https_address().await?.port())
            .await?;

        info!("Homeserver listening on https://{}", core.public_key());

        Ok(Self { http_servers, core })
    }

    /// Test version of [Homeserver::start], using mainline Testnet, and a temporary storage.
    pub async fn start_test(testnet: &Testnet) -> Result<Self> {
        info!("Running testnet..");

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
