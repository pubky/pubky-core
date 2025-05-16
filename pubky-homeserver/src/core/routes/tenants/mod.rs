//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, put},
    Router,
};

use crate::core::{layers::authz::AuthorizationLayer, AppState};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // - Datastore routes
        .route("{*path}", get(read::get))
        .route("{*path}", head(read::head))
        .route("{*path}", put(write::put))
        .route("{*path}", delete(write::delete))
        // .route("/pub/", get(read::get))
        // .route("/pub/{*path}", get(read::get))
        // .route("/pub/{*path}", head(read::head))
        // .route("/pub/{*path}", put(write::put))
        // .route("/pub/{*path}", delete(write::delete))
        // - Session routes
        .route("/session", get(session::session))
        .route("/session", delete(session::signout))
        // Layers
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(AuthorizationLayer::new(state.clone()))
}
