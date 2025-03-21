//! Http server around the HomeserverCore

use std::{
    net::{SocketAddr, TcpListener},
    sync::Arc,
};

use anyhow::Result;
use axum::Router;
use axum_server::{
    tls_rustls::{RustlsAcceptor, RustlsConfig},
    Handle,
};
use futures_util::TryFutureExt;
use pkarr::Keypair;

use super::IoConfig;

#[derive(Debug)]
pub struct HttpServers {
    /// Handle for the HTTP server
    pub(crate) http_handle: Handle,
    /// Handle for the HTTPS server using Pkarr TLS
    pub(crate) https_handle: Handle,

    http_address: SocketAddr,
    https_address: SocketAddr,
}

impl HttpServers {
    pub async fn run(keypair: &Keypair, config: &IoConfig, router: &Router) -> Result<Self> {
        let http_listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.http_port)))?;
        let http_address = http_listener.local_addr()?;

        let http_handle = Handle::new();

        tokio::spawn(
            axum_server::from_tcp(http_listener)
                .handle(http_handle.clone())
                .serve(
                    router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver http server error")),
        );

        let https_listener =
            TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.https_port)))?;
        let https_address = https_listener.local_addr()?;

        let https_handle = Handle::new();

        tokio::spawn(
            axum_server::from_tcp(https_listener)
                .acceptor(RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
                    keypair.to_rpk_rustls_server_config(),
                ))))
                .handle(https_handle.clone())
                .serve(
                    router
                        .clone()
                        .into_make_service_with_connect_info::<SocketAddr>(),
                )
                .map_err(|error| tracing::error!(?error, "Homeserver https server error")),
        );

        Ok(Self {
            http_handle,
            https_handle,

            http_address,
            https_address,
        })
    }

    pub fn http_address(&self) -> SocketAddr {
        self.http_address
    }

    pub fn https_address(&self) -> SocketAddr {
        self.https_address
    }

    /// Shutdown all HTTP servers.
    pub fn shutdown(&self) {
        self.http_handle.shutdown();
        self.https_handle.shutdown();
    }
}
