use std::{future::IntoFuture, net::SocketAddr};

use anyhow::{Error, Result};
use tokio::{net::TcpListener, signal, task::JoinSet};
use tracing::{info, warn};

use pkarr::{
    mainline::dht::{DhtSettings, Testnet},
    PkarrClient, PublicKey, Settings,
};

use crate::{config::Config, pkarr::publish_server_packet};

#[derive(Debug)]
pub struct Homeserver {
    pub(crate) config: Config,
    port: u16,
    tasks: JoinSet<std::io::Result<()>>,
}

impl Homeserver {
    pub async fn start(config: Config) -> Result<Self> {
        let app = crate::routes::create_app();

        let mut tasks = JoinSet::new();

        let app = app.clone();

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.port()))).await?;

        let port = listener.local_addr()?.port();

        // Spawn http server task
        tasks.spawn(
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal())
            .into_future(),
        );

        info!("Homeserver listening on http://localhost:{port}");

        let pkarr_client = PkarrClient::new(Settings {
            dht: DhtSettings {
                bootstrap: config.bootstsrap(),
                ..Default::default()
            },
            ..Default::default()
        })?
        .as_async();

        publish_server_packet(pkarr_client, config.keypair(), config.domain(), port).await?;

        info!(
            "Homeserver listening on pubky://{}",
            config.keypair().public_key()
        );

        Ok(Self {
            tasks,
            config,
            port,
        })
    }

    /// Test version of [Homeserver::start], using mainline Testnet, and a temporary storage.
    pub async fn start_test(testnet: &Testnet) -> Result<Self> {
        Homeserver::start(Config::test(testnet)).await
    }

    // === Getters ===

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn public_key(&self) -> PublicKey {
        self.config.keypair().public_key()
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

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    fn graceful_shutdown() {
        info!("Gracefully Shutting down..");
    }

    tokio::select! {
        _ = ctrl_c => graceful_shutdown(),
        _ = terminate => graceful_shutdown(),
    }
}
