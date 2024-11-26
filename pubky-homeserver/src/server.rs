use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::{Error, Result};
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use pubky_common::auth::AuthVerifier;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use pkarr::{mainline::Testnet, PublicKey};

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
    pub(crate) pkarr_client: pkarr::Client,
    pub(crate) config: Config,
    pub(crate) port: u16,
}

impl Homeserver {
    pub async fn start(config: Config) -> Result<Self> {
        debug!(?config);

        let db = DB::open(config.clone())?;

        let mut dht_settings = pkarr::mainline::Settings::default();

        if let Some(bootstrap) = config.bootstrap() {
            dht_settings = dht_settings.bootstrap(&bootstrap);
        }
        if let Some(request_timeout) = config.dht_request_timeout() {
            dht_settings = dht_settings.request_timeout(request_timeout);
        }

        let pkarr_client = pkarr::Client::builder()
            .dht_settings(dht_settings)
            .build()?;

        let mut tasks = JoinSet::new();

        let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.port())))?;

        let port = listener.local_addr()?.port();

        let state = AppState {
            verifier: AuthVerifier::default(),
            db,
            pkarr_client: pkarr_client.clone(),
            config: config.clone(),
            port,
        };

        let acceptor = RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
            config.keypair().to_rpk_rustls_server_config(),
        )));
        let server = axum_server::from_tcp(listener).acceptor(acceptor);

        let app = crate::routes::create_app(state.clone());

        // Spawn http server task
        tasks.spawn(server.serve(app.into_make_service_with_connect_info::<SocketAddr>()));

        info!("Homeserver listening on http://localhost:{port}");

        info!("Publishing Pkarr packet..");

        publish_server_packet(&pkarr_client, &config, port).await?;

        info!(
            "Homeserver listening on https://{}",
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

    #[cfg(test)]
    pub(crate) fn database_mut(&mut self) -> &mut DB {
        &mut self.state.db
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
