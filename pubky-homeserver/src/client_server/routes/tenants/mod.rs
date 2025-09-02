//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{extract::DefaultBodyLimit, routing::get, Router};

use crate::client_server::{
    layers::authz::AuthorizationLayer, layers::pubky_host::PubkyHostLayer, AppState,
};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/session", get(session::session).delete(session::signout))
        .route(
            "/{*path}",
            get(read::get)
                .head(read::head)
                .put(write::put)
                .delete(write::delete),
        )
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(AuthorizationLayer::new(state.clone()))
        .layer(PubkyHostLayer)
}

pub fn webdav_router(state: AppState) -> Router<AppState> {
    // TODO: layers for:
    // - webdav auth (http basic)
    // - anything else?
    Router::new()
        .route(
            "/{key}/{*path}",
            get(read::get)
                .head(read::head)
                .put(write::put)
                .delete(write::delete),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        // FIXME
        .layer(AuthorizationLayer::new(state.clone()))
}
