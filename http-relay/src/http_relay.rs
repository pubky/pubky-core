//! https://httprelay.io/features/link/

use std::{
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
use tokio::sync::Mutex;

use tower_http::{cors::CorsLayer, trace::TraceLayer};
use url::Url;

use crate::waiting_list::WaitingList;

/// The timeout for a request to be considered unused.
/// This is to prevent memory leaks and to keep the server responsive.
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone)]
struct AppState {
    pub config: Config,
    pub pending_list: Arc<Mutex<WaitingList>>,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            pending_list: Arc::new(Mutex::new(WaitingList::default())),
        }
    }
}

#[derive(Debug, Clone)]
struct Config {
    pub http_port: u16,
    pub request_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_port: 0,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        }
    }
}

/// Builder for [HttpRelay].
#[derive(Debug, Default)]
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
    pub(crate) http_handle: Handle<SocketAddr>,
    http_address: SocketAddr,
}

impl HttpRelay {
    /// Creates the HTTP router for the HTTP relay.
    /// Extracted as its own function to make it easier to test.
    fn create_app(config: Config) -> Result<(Router, AppState)> {
        let app_state = AppState::new(config);

        let app = Router::new()
            .route(
                "/link/{id}",
                get(link::get_handler).post(link::post_handler),
            )
            .layer(CorsLayer::very_permissive())
            .layer(TraceLayer::new_for_http())
            .with_state(app_state.clone());

        Ok((app, app_state))
    }

    async fn start(config: Config) -> Result<Self> {
        let (app, _) = Self::create_app(config.clone())?;

        let http_handle = Handle::new();
        let shutdown_handle = http_handle.clone();

        let http_listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], config.http_port)))?;
        http_listener.set_nonblocking(true)?;
        let http_address = http_listener.local_addr()?;

        let server = axum_server::from_tcp(http_listener)?;
        tokio::spawn(async move {
            server
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
    use super::*;
    use axum::http::StatusCode;

    /// A consumer requests data using GET method.
    pub async fn get_handler(
        Path(id): Path<String>,
        State(state): State<AppState>,
    ) -> impl IntoResponse {
        let mut pending_list = state.pending_list.lock().await;

        if let Some(producer) = pending_list.remove_producer(&id) {
            // Producer is ready to send data
            let _ = producer.completion.send(());
            return (StatusCode::OK, producer.body);
        };

        // No producer ready. Insert consumer into pending list and wait for producer to send data.
        let receiver = pending_list.insert_consumer(&id);
        drop(pending_list);

        // Wait for the producer, but with a timeout
        match tokio::time::timeout(state.config.request_timeout, receiver).await {
            Ok(Ok(message)) => (StatusCode::OK, message),
            Ok(Err(_)) => (StatusCode::NOT_FOUND, "Not Found".into()),
            Err(_) => {
                // Timeout. Remove the consumer from the pending list again
                let mut pending_list = state.pending_list.lock().await;
                pending_list.remove_consumer(&id);
                (StatusCode::REQUEST_TIMEOUT, "Request timed out".into())
            }
        }
    }

    /// A producer sends data using POST method.
    pub async fn post_handler(
        Path(channel): Path<String>,
        State(state): State<AppState>,
        body: Bytes,
    ) -> impl IntoResponse {
        let mut pending_list = state.pending_list.lock().await;

        if let Some(consumer) = pending_list.remove_consumer(&channel) {
            // Consumer is ready to receive data
            let _ = consumer.message_sender.send(body);
            return (StatusCode::OK, Bytes::new());
        };

        // No consumer ready. Insert producer into pending list and wait for consumer to request data.
        let receiver = pending_list.insert_producer(&channel, body);
        drop(pending_list);
        match tokio::time::timeout(state.config.request_timeout, receiver).await {
            Ok(_) => (StatusCode::OK, Bytes::new()),
            Err(_) => {
                // Timeout. Remove the producer from the pending list again
                let mut pending_list = state.pending_list.lock().await;
                pending_list.remove_producer(&channel);
                (StatusCode::REQUEST_TIMEOUT, "Request timed out".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_delayed_producer() {
        let (app, state) = HttpRelay::create_app(Config::default()).unwrap();
        let server = axum_test::TestServer::new(app).unwrap();

        let consumer = async {
            let response = server.get("/link/123").await;
            assert_eq!(response.status_code(), 200);
            assert_eq!(response.text(), "Hello, world!");
        };

        let producer = async {
            tokio::time::sleep(Duration::from_millis(200)).await; // Delayed produce to ensure consumer is waiting
            let body = axum::body::Bytes::from_static(b"Hello, world!");
            let response = server.post("/link/123").bytes(body).await;
            assert_eq!(response.status_code(), 200);
            assert_eq!(response.text(), "");
        };

        tokio::join!(consumer, producer);
        assert!(state.pending_list.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_delayed_consumer() {
        let (app, state) = HttpRelay::create_app(Config::default()).unwrap();
        let server = axum_test::TestServer::new(app).unwrap();

        let consumer = async {
            tokio::time::sleep(Duration::from_millis(200)).await; // Delayed consumer to ensure producer is waiting
            let response = server.get("/link/123").await;
            assert_eq!(response.status_code(), 200);
            assert_eq!(response.text(), "Hello, world!");
        };

        let producer = async {
            let body = axum::body::Bytes::from_static(b"Hello, world!");
            let response = server.post("/link/123").bytes(body).await;
            assert_eq!(response.status_code(), 200);
            assert_eq!(response.text(), "");
        };

        tokio::join!(consumer, producer);
        assert!(state.pending_list.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_request_timeout() {
        let config = Config {
            request_timeout: Duration::from_millis(50),
            ..Config::default()
        };
        let (app, state) = HttpRelay::create_app(config).unwrap();
        let server = axum_test::TestServer::new(app).unwrap();

        // Consumer request timed out
        let response = server.get("/link/123").await;
        assert_eq!(response.status_code(), 408);
        assert_eq!(response.text(), "Request timed out");
        assert!(state.pending_list.lock().await.is_empty());

        // Producer request timed out
        let body = axum::body::Bytes::from_static(b"Hello, world!");
        let response = server.post("/link/123").bytes(body).await;
        assert_eq!(response.status_code(), 408);
        assert_eq!(response.text(), "Request timed out");
        assert!(state.pending_list.lock().await.is_empty());
    }
}
