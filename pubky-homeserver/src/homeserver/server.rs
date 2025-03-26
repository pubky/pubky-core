use std::{net::SocketAddr, path::PathBuf, time::Duration};

use super::http::HttpServers;
use crate::{context::AppContext, data_directory::DataDir, SignupMode};
use anyhow::Result;
use pkarr::{Keypair, PublicKey};
use tracing::info;

use crate::core::{CoreConfig, HomeserverCore};

pub const DEFAULT_HTTP_PORT: u16 = 6286;
pub const DEFAULT_HTTPS_PORT: u16 = 6287;


#[derive(Debug)]
/// Homeserver Core + I/O (http server and pkarr publishing).
pub struct Homeserver {
    http_servers: HttpServers,
    keypair: Keypair,
}

impl Homeserver {

    /// Run the homeserver with configurations from a data directory.
    pub async fn run_with_data_dir(dir_path: PathBuf) -> Result<Self> {
        let data_dir = DataDir::new(dir_path);
        let context = AppContext::try_from(data_dir)?;
        Self::run(context).await
    }

    /// Run a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    async fn run(context: AppContext) -> Result<Self> {
        tracing::debug!(?context, "Running homeserver with configurations");

        let core = HomeserverCore::new(&context).await?;

        // let http_servers = HttpServers::run(&keypair, &config.io, &core.router).await?;

        // let admin_server = run_admin_server(ase, config.admin.password.as_str(), config.admin.listen).await?;


        info!(
            "Homeserver listening on http://localhost:{}",
            http_servers.http_address().port()
        );
        info!("Homeserver listening on https://{}", keypair.public_key());

        Ok(Self {
            http_servers,
            keypair,
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
    }
}
