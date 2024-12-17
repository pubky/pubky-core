use std::{
    collections::HashMap,
    net::SocketAddr,
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

use futures_util::stream::StreamExt;
use url::Url;

// Shared state to store GET requests and their notifications
type SharedState = Arc<Mutex<HashMap<String, (Vec<u8>, Arc<Notify>)>>>;

pub struct HttpRelay {
    pub(crate) http_handle: Handle,
}

impl HttpRelay {
    pub async fn start() -> Result<Self> {
        let shared_state: SharedState = Arc::new(Mutex::new(HashMap::new()));

        let app = Router::new()
            .route("/link/:id", get(link::get).post(link::post))
            .with_state(shared_state);

        let http_handle = Handle::new();

        let cloned = http_handle.clone();
        tokio::spawn(async {
            axum_server::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .handle(cloned)
                .serve(app.into_make_service())
                .await
                .unwrap();
        });

        Ok(Self { http_handle })
    }

    pub async fn http_address(&self) -> Result<SocketAddr> {
        match self.http_handle.listening().await {
            Some(addr) => Ok(addr),
            None => Err(anyhow::anyhow!("Failed to bind to http port")),
        }
    }

    /// Returns the localhost Url of this server.
    pub async fn local_url(&self) -> Result<Url> {
        match self.http_handle.listening().await {
            Some(addr) => Ok(Url::parse(&format!("http://localhost:{}", addr.port()))?),
            None => Err(anyhow::anyhow!("Failed to bind to http port")),
        }
    }

    /// Returns the localhost URL of Link endpoints
    pub async fn local_link_url(&self) -> Result<Url> {
        let mut url = self.local_url().await?;

        let mut segments = url
            .path_segments_mut()
            .expect("HttpRelay::local_link_url path_segments_mut");

        segments.push("link");

        drop(segments);

        Ok(url)
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
