use std::{future::IntoFuture, net::SocketAddr};

use anyhow::{Error, Result};
use pkarr::{mainline::dht::DhtSettings, PkarrClient, PkarrClientAsync, Settings};
use tokio::{net::TcpListener, signal, task::JoinSet};
use tracing::{debug, info, warn};

use pkarr::mainline::Testnet;

use pk_common::crypto::{Keypair, PublicKey};

use crate::config::Config;
use crate::db::DB;
use crate::pkarr::publish_server_pkarr;

#[derive(Debug)]
pub struct Homeserver {
    state: AppState,
    tasks: JoinSet<std::io::Result<()>>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub public_key: PublicKey,
    pub pkarr_client: PkarrClientAsync,
    pub db: DB,
}

impl Homeserver {
    pub async fn start(config: Config) -> Result<Self> {
        let keypair = Keypair::random();

        let state = AppState {
            public_key: keypair.public_key(),
            pkarr_client: PkarrClient::new(Settings {
                dht: DhtSettings {
                    bootstrap: config.bootstsrap(),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })?
            .as_async(),
            db: DB::open(&config.storage()?)?,
        };

        debug!(?config);

        let app = crate::routes::create_app(state.clone());

        let mut tasks = JoinSet::new();

        let app = app.clone();

        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], config.port()))).await?;

        let bound_addr = listener.local_addr()?;

        // Spawn http server task
        tasks.spawn(
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(shutdown_signal())
            .into_future(),
        );

        info!("HTTP server listening on {bound_addr}");

        publish_server_pkarr(
            &state.pkarr_client,
            &keypair,
            config.domain(),
            bound_addr.port(),
        )
        .await?;

        info!("HTTP server listening on pk:{}", state.public_key);

        Ok(Self { state, tasks })
    }

    // === Getters ===

    pub fn public_key(&self) -> &PublicKey {
        &self.state.public_key
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

    /// Test version of [Homeserver::start], using mainline Testnet, and a temporary storage.
    pub async fn start_test(testnet: &Testnet) -> Result<Self> {
        Homeserver::start(Config::test(&testnet)).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn basic() {
        let testnet = Testnet::new(3);
        let _ = Homeserver::start_test(&testnet).await.unwrap();
    }
}
