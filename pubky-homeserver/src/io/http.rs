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

use crate::core::HomeserverCore;

pub(crate) async fn start(core: HomeserverCore) -> Result<Handle> {
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], core.config.port())))?;

    let acceptor = RustlsAcceptor::new(RustlsConfig::from_config(Arc::new(
        core.keypair().to_rpk_rustls_server_config(),
    )));
    let server = axum_server::from_tcp(listener).acceptor(acceptor);

    let handle = Handle::new();

    tokio::spawn(
        server.handle(handle.clone()).serve(
            core.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        ),
    );

    Ok(handle)
}
