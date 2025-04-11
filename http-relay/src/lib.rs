//! A Rust implementation of _some_ of [Http relay spec](https://httprelay.io/).
//!

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(any(), deny(clippy::unwrap_used))]

use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;

use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use axum_server::Handle;
use tokio::sync::{oneshot, Mutex};

use tower_http::{cors::CorsLayer, trace::TraceLayer};
use url::Url;

// Shared state to store GET requests and their notifications
type SharedState = Arc<Mutex<HashMap<String, ChannelState>>>;

enum ChannelState {
    ProducerWaiting {
        body: Bytes,
        completion: oneshot::Sender<()>,
    },
    ConsumerWaiting {
        message_sender: oneshot::Sender<Bytes>,
    },
}

#[derive(Debug, Default)]
struct Config {
    pub http_port: u16,
}

#[derive(Debug, Default)]
/// Builder for [HttpRelay].
pub struct HttpRelayBuilder(Config);

impl HttpRelayBuilder {
    /// Configure the port used for HTTP server.
    pub fn http_port(mut self, port: u16) -> Self {
        self.0.http_port = port;

        self
    }

    /// Start running an HTTP relay.
    pub async fn run(self) -> Result<HttpRelay> {
        HttpRelay::start(self.0).await
    }
}

/// An implementation of _some_ of [Http relay spec](https://httprelay.io/).
pub struct HttpRelay {
    pub(crate) http_handle: Handle,
    http_address: SocketAddr,
}

impl HttpRelay {
    async fn start(config: Config) -> Result<Self> {
        let shared_state: SharedState = Arc::new(Mutex::new(HashMap::new()));

        let app = Router::new()
            .route("/link/{id}", get(link::get).post(link::post))
            .layer(CorsLayer::very_permissive())
            .layer(TraceLayer::new_for_http())
            .with_state(shared_state);

        let http_handle = Handle::new();
        let shutdown_handle = http_handle.clone();

        let http_listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.http_port)))?;
        let http_address = http_listener.local_addr()?;

        tokio::spawn(async move {
            axum_server::from_tcp(http_listener)
                .handle(http_handle.clone())
                .serve(app.into_make_service())
                .await
                .map_err(|error| tracing::error!(?error, "HttpRelay http server error"))
        });

        Ok(Self {
            http_handle: shutdown_handle,
            http_address,
        })
    }

    /// Create [HttpRelayBuilder].
    pub fn builder() -> HttpRelayBuilder {
        HttpRelayBuilder::default()
    }

    /// Returns the HTTP address of this http relay.
    pub fn http_address(&self) -> SocketAddr {
        self.http_address
    }

    /// Returns the localhost Url of this server.
    pub fn local_url(&self) -> Url {
        Url::parse(&format!("http://localhost:{}", self.http_address.port()))
            .expect("local_url should be formatted fine")
    }

    /// Returns the localhost URL of Link endpoints
    pub fn local_link_url(&self) -> Url {
        let mut url = self.local_url();

        let mut segments = url
            .path_segments_mut()
            .expect("HttpRelay::local_link_url path_segments_mut");

        segments.push("link");

        drop(segments);

        url
    }

    /// Gracefully shuts down the HTTP relay.
    pub async fn shutdown(self) -> anyhow::Result<()> {
        self.http_handle
            .graceful_shutdown(Some(Duration::from_secs(1)));
        Ok(())
    }
}

impl Drop for HttpRelay {
    fn drop(&mut self) {
        self.http_handle.shutdown();
    }
}

mod link {
    use axum::http::StatusCode;

    use super::*;

    pub async fn get(
        Path(id): Path<String>,
        State(state): State<SharedState>,
    ) -> impl IntoResponse {
        let mut channels = state.lock().await;

        match channels.remove(&id) {
            Some(ChannelState::ProducerWaiting { body, completion }) => {
                let _ = completion.send(());

                (StatusCode::OK, body)
            }
            _ => {
                let (message_sender, message_receiver) = oneshot::channel();
                channels.insert(id, ChannelState::ConsumerWaiting { message_sender });
                drop(channels);

                match message_receiver.await {
                    Ok(message) => (StatusCode::OK, message),
                    Err(_) => (StatusCode::NOT_FOUND, "Not Found".into()),
                }
            }
        }
    }

    pub async fn post(
        Path(channel): Path<String>,
        State(state): State<SharedState>,
        body: Bytes,
    ) -> impl IntoResponse {
        let mut channels = state.lock().await;

        match channels.remove(&channel) {
            Some(ChannelState::ConsumerWaiting { message_sender }) => {
                let _ = message_sender.send(body);
                (StatusCode::OK, ())
            }
            _ => {
                let (completion_sender, completion_receiver) = oneshot::channel();
                channels.insert(
                    channel,
                    ChannelState::ProducerWaiting {
                        body,
                        completion: completion_sender,
                    },
                );
                drop(channels);
                let _ = completion_receiver.await;
                (StatusCode::OK, ())
            }
        }
    }
}
