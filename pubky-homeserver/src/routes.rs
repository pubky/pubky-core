use axum::{
    routing::{get, post, put},
    Router,
};
use tower_http::trace::TraceLayer;

use crate::server::AppState;

mod auth;
mod drive;
mod root;

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::handler))
        .route("/:pubky", put(auth::signup))
        .route("/:pubky/*key", get(drive::put))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
