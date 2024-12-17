use std::{
    collections::HashMap,
    net::{SocketAddr, TcpListener},
    sync::{Arc, Mutex},
};

use anyhow::Result;

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Router,
};
use axum_server::Handle;
use tokio::sync::Notify;

use futures_util::{stream::StreamExt, TryFutureExt};
use url::Url;

// Shared state to store GET requests and their notifications
type SharedState = Arc<Mutex<HashMap<String, (Vec<u8>, Arc<Notify>)>>>;

#[derive(Debug, Default)]
pub struct Config {
    pub http_port: u16,
}

#[derive(Debug, Default)]
pub struct HttpRelayBuilder(Config);

impl HttpRelayBuilder {
    /// Configure the port used for HTTP server.
    pub fn http_port(mut self, port: u16) -> Self {
        self.0.http_port = port;

        self
    }

    pub async fn build(self) -> Result<HttpRelay> {
        HttpRelay::start(self.0).await
    }
}

pub struct HttpRelay {
    pub(crate) http_handle: Handle,

    http_address: SocketAddr,
}

impl HttpRelay {
    pub fn builder() -> HttpRelayBuilder {
        HttpRelayBuilder::default()
    }

    pub async fn start(config: Config) -> Result<Self> {
        let shared_state: SharedState = Arc::new(Mutex::new(HashMap::new()));

        let app = Router::new()
            .route("/link/:id", get(link::get).post(link::post))
            .with_state(shared_state);

        let http_handle = Handle::new();

        let http_listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.http_port)))?;
        let http_address = http_listener.local_addr()?;

        tokio::spawn(
            axum_server::from_tcp(http_listener)
                .handle(http_handle.clone())
                .serve(app.into_make_service())
                .map_err(|error| tracing::error!(?error, "HttpRelay http server error")),
        );

        Ok(Self {
            http_handle,
            http_address,
        })
    }

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

    pub fn shutdown(&self) {
        self.http_handle.shutdown();
    }
}

mod link {
    use super::*;

    pub async fn get(
        Path(id): Path<String>,
        State(state): State<SharedState>,
    ) -> impl IntoResponse {
        // Create a notification for this ID
        let notify = Arc::new(Notify::new());

        {
            let mut map = state.lock().unwrap();

            // Store the notification and return it when POST arrives
            map.entry(id.clone())
                .or_insert_with(|| (vec![], notify.clone()));
        }

        notify.notified().await;

        // Respond with the data stored for this ID
        let map = state.lock().unwrap();
        if let Some((data, _)) = map.get(&id) {
            Bytes::from(data.clone()).into_response()
        } else {
            (axum::http::StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }

    pub async fn post(
        Path(id): Path<String>,
        State(state): State<SharedState>,
        body: Body,
    ) -> impl IntoResponse {
        // Aggregate the body into bytes
        let mut stream = body.into_data_stream();
        let mut bytes = vec![];
        while let Some(next) = stream.next().await {
            let chunk = next.map_err(|e| e.to_string()).unwrap();
            bytes.extend_from_slice(&chunk);
        }

        // Notify any waiting GET request for this ID
        let mut map = state.lock().unwrap();
        if let Some((storage, notify)) = map.get_mut(&id) {
            *storage = bytes;
            notify.notify_one();
            Ok(())
        } else {
            Err((
                axum::http::StatusCode::NOT_FOUND,
                "No waiting GET request for this ID",
            ))
        }
    }
}
