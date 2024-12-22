//! The controller part of the [crate::HomeserverCore]

use axum::{
    routing::{get, post},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::core::AppState;

mod auth;
mod feed;
mod root;
mod tenants;

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
    base()
        .merge(tenants::router(state.clone()))
        .layer(CookieManagerLayer::new())
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
