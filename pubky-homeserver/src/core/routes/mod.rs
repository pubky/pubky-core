//! The controller part of the [crate::HomeserverCore]

use axum::{
    routing::{get, post},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::cors::CorsLayer;

use crate::core::AppState;

use super::layers::trace::with_trace_layer;

mod auth;
mod feed;
mod root;
mod tenants;

const TRACING_EXCLUDED_PATHS: [&str; 1] = ["/events/"];

fn base() -> Router<AppState> {
    Router::new()
        .route("/", get(root::handler))
        .route("/signup", post(auth::signup))
        .route("/session", post(auth::signin))
        // Events
        .route("/events/", get(feed::feed))
    // TODO: add size limit
    // TODO: revisit if we enable streaming big payloads
    // TODO: maybe add to a separate router (drive router?).
}

pub fn create_app(state: AppState) -> Router {
    let app = base()
        .merge(tenants::router(state.clone()))
        .layer(CookieManagerLayer::new())
        .layer(CorsLayer::very_permissive())
        .with_state(state);

    with_trace_layer(app, &TRACING_EXCLUDED_PATHS)
}
