use std::{future::IntoFuture, net::SocketAddr};

use anyhow::{Error, Result};
use tokio::{net::TcpListener, signal, task::JoinSet};
use tracing::{info, warn};

#[derive(Debug)]
pub struct Homeserver {
    tasks: JoinSet<std::io::Result<()>>,
}

impl Homeserver {
    pub async fn start() -> Result<Self> {
        let app = crate::routes::create_app();

        let mut tasks = JoinSet::new();

        let app = app.clone();

        let listener = TcpListener::bind(SocketAddr::from((
            [127, 0, 0, 1],
            6287, // config.port()
        )))
        .await?;

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

        Ok(Self { tasks })
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
