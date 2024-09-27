use std::{future::IntoFuture, net::SocketAddr};

use anyhow::{Error, Result};
use pubky_common::auth::AuthVerifier;
use tokio::{net::TcpListener, signal, task::JoinSet};
use tracing::{debug, info, warn};

use pkarr::{
    mainline::dht::{DhtSettings, Testnet},
    PkarrClient, PkarrClientAsync, PublicKey, Settings,
};

use crate::{config::Config, database::DB, pkarr::publish_server_packet};

#[derive(Debug)]
pub struct Homeserver {
    state: AppState,
    tasks: JoinSet<std::io::Result<()>>,
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    pub(crate) db: DB,
    pub(crate) pkarr_client: PkarrClientAsync,
    pub(crate) config: Config,
    pub(crate) port: u16,
}

impl Homeserver {
    pub async fn start(config: Config) -> Result<Self> {
        debug!(?config);

        let db = DB::open(config.clone())?;

        let pkarr_client = PkarrClient::new(Settings {
            dht: DhtSettings {
                bootstrap: config.bootstsrap(),
                request_timeout: config.dht_request_timeout(),
                ..Default::default()
            },
            ..Default::default()
        })?
        .as_async();

        let mut tasks = JoinSet::new();

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.port()))).await?;

        let port = listener.local_addr()?.port();

        let state = AppState {
            verifier: AuthVerifier::default(),
            db,
            pkarr_client,
            config: config.clone(),
            port,
        };

        let app = crate::routes::create_app(state.clone());

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

        publish_server_packet(
            &state.pkarr_client,
            config.keypair(),
            &state
                .config
                .domain()
                .clone()
                .unwrap_or("localhost".to_string()),
            port,
        )
        .await?;

        info!(
            "Homeserver listening on pubky://{}",
            config.keypair().public_key()
        );

        Ok(Self { tasks, state })
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
