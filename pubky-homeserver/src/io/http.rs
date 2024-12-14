//! Http server around the HomeserverCore

use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::Result;
use axum_server::{
    tls_rustls::{RustlsAcceptor, RustlsConfig},
    Handle,
};
use futures_util::TryFutureExt;

use crate::core::HomeserverCore;

#[derive(Debug)]
pub struct HttpServers {
    /// Handle for the HTTP server
    pub(crate) http_handle: Handle,
    /// Handle for the HTTPS server using Pkarr TLS
    pub(crate) https_handle: Handle,
    // /// Handle for a mock relay used in testnet
    // pub(crate) mock_pkarr_relay_handle: Handle,
}

impl HttpServers {
    pub async fn start(core: &HomeserverCore) -> Result<Self> {
        let http_listener =
        // TODO: add config to http port
            TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0)))?;

        let http_handle = Handle::new();

        tokio::spawn(
            axum_server::from_tcp(http_listener)
                .handle(http_handle.clone())
                .serve(
                    core.router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver http server error")),
        );

        let https_listener =
            TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], core.config().port)))?;

        let https_handle = Handle::new();

        tokio::spawn(
            axum_server::from_tcp(https_listener)
                .acceptor(RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
                    core.keypair().to_rpk_rustls_server_config(),
                ))))
                .handle(https_handle.clone())
                .serve(
                    core.router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver https server error")),
        );

        // let mock_pkarr_relay_listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 15411)))?;

        Ok(Self {
            http_handle,
            https_handle,
        })
    }

    pub async fn http_address(&self) -> Result<SocketAddr> {
        match self.http_handle.listening().await {
            Some(addr) => Ok(addr),
            None => Err(anyhow::anyhow!("Failed to bind to http port")),
        }
    }

    pub async fn https_address(&self) -> Result<SocketAddr> {
        match self.https_handle.listening().await {
            Some(addr) => Ok(addr),
            None => Err(anyhow::anyhow!("Failed to bind to https port")),
        }
    }

    /// Shutdown all HTTP servers.
    pub fn shutdown(&self) {
        self.http_handle.shutdown();
        self.https_handle.shutdown();
    }
}
