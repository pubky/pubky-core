use std::net::SocketAddr;

use axum::{response::IntoResponse, routing::get, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_sessions::{cookie::SameSite, MemoryStore, Session, SessionManagerLayer};

const COUNTER_KEY: &str = "counter";

#[derive(Default, Deserialize, Serialize)]
struct Counter(usize);

async fn handler(session: Session) -> Result<impl IntoResponse, String> {
    let counter: Counter = session
        .get(COUNTER_KEY)
        .await
        .map_err(|_| "Error")?
        .unwrap_or_default();
    session
        .insert(COUNTER_KEY, counter.0 + 1)
        .await
        .map_err(|_| "Error2")?;
    Ok(format!("Current count: {}", counter.0))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(handler))
        .layer(SessionManagerLayer::new(MemoryStore::default()).with_same_site(SameSite::None))
        .layer(CorsLayer::very_permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
