//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, put},
    Router,
};

use crate::{core::{layers::{authz::AuthorizationLayer, rate_limiter::RateLimiterLayer}, AppState}, quota_config::QuotaConfig};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    let config: QuotaConfig = "ip:1kb/m".parse().unwrap();
    Router::new()
        // - Datastore routes
        .route("/pub/", get(read::get))
        .route("/pub/{*path}", get(read::get))
        .route("/pub/{*path}", head(read::head))
        .route("/pub/{*path}", put(write::put).layer(RateLimiterLayer::new(Some(config))))
        .route("/pub/{*path}", delete(write::delete))
        // - Session routes
        .route("/session", get(session::session))
        .route("/session", delete(session::signout))
        // Layers
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(AuthorizationLayer::new(state.clone()))
}
