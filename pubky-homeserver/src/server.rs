use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::{Error, Result};
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use tokio::task::JoinSet;
use tracing::{info, warn};

use pkarr::{mainline::Testnet, PublicKey};

use crate::{
    config::Config,
    core::{AppState, HomeserverCore},
    pkarr::publish_server_packet,
};

#[derive(Debug)]
/// Homeserver [Core][HomeserverCore] + http server.
pub struct Homeserver {
    state: AppState,
    tasks: JoinSet<std::io::Result<()>>,
}

impl Homeserver {
    pub async fn start(config: Config) -> Result<Self> {
        let mut tasks = JoinSet::new();

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.port())))?;

        let port = listener.local_addr()?.port();

        let keypair = config.keypair().clone();

        let core = HomeserverCore::new(&config)?;

        let acceptor = RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
            keypair.to_rpk_rustls_server_config(),
        )));
        let server = axum_server::from_tcp(listener).acceptor(acceptor);

        // Spawn http server task
        tasks.spawn(
            server.serve(
                core.router
                    .into_make_service_with_connect_info::<SocketAddr>(),
            ),
        );

        info!("Homeserver listening on http://localhost:{port}");

        info!("Publishing Pkarr packet..");

        publish_server_packet(&core.state.pkarr_client, &config, port).await?;

        info!("Homeserver listening on https://{}", keypair.public_key());

        Ok(Self {
            tasks,
            state: core.state,
        })
    }

    /// Test version of [Homeserver::start], using mainline Testnet, and a temporary storage.
    pub async fn start_test(testnet: &Testnet) -> Result<Self> {
        info!("Running testnet..");

        Homeserver::start(Config::test(testnet)).await
    }

    // === Getters ===

    pub fn port(&self) -> u16 {
        self.state.port
    }

    pub fn public_key(&self) -> PublicKey {
        self.state.config.keypair().public_key()
    }

    /// Return the `https://<server public key>` url
    pub fn url(&self) -> url::Url {
        url::Url::parse(&format!("https://{}", self.public_key())).expect("valid url")
    }

    // === Public Methods ===

    /// Shutdown the server and wait for all tasks to complete.
    pub async fn shutdown(mut self) -> Result<()> {
        self.tasks.abort_all();
        self.run_until_done().await?;
        Ok(())
    }

    /// Wait for all tasks to complete.
    ///
    /// Runs forever unless tasks fail.
    pub async fn run_until_done(mut self) -> Result<()> {
        let mut final_res: Result<()> = Ok(());
        while let Some(res) = self.tasks.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Err(err) if err.is_cancelled() => {}
                Ok(Err(err)) => {
                    warn!(?err, "task failed");
                    final_res = Err(Error::from(err));
                }
                Err(err) => {
                    warn!(?err, "task panicked");
                    final_res = Err(err.into());
                }
            }
        }
        final_res
    }
}
