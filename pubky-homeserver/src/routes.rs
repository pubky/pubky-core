use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;

pub mod drive;
pub mod root;

pub fn create_app() -> Router {
    Router::new()
        .route("/", get(root::handler))
        .route("/:pubky/*key", get(drive::put))
        .layer(TraceLayer::new_for_http())
}
