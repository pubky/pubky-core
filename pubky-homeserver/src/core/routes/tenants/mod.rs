//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, put},
    Router,
};

use crate::core::{
    layers::{authz::AuthorizationLayer, pubky_host::PubkyHostLayer},
    AppState,
};

use super::auth;

pub mod read;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        // - Datastore routes
        .route("/pub/", get(read::get))
        .route("/pub/*path", get(read::get))
        .route("/pub/*path", head(read::head))
        .route("/pub/*path", put(write::put))
        .route("/pub/*path", delete(write::delete))
        // - Session routes
        .route("/session", get(auth::session))
        .route("/session", delete(auth::signout))
        // Layers
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(AuthorizationLayer::new(state.clone()))
        .layer(PubkyHostLayer)
}
