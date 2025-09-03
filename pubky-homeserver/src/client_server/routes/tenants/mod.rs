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
    Router::new()
        .route(
            "/{key}/{*path}",
            // FIXME use state.inner_dav_handler.handle
            get(read::get)
                .head(read::head)
                .put(write::put)
                .delete(write::delete),
        )
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        // NOTE observed that admin's dav auth is managed by `routes::dav_handler.rs`
        // For now I think it is better to keep it in Layer
        // TODO: layers for webdav supported auth:
        // - Basic Authentication https://www.rfc-editor.org/rfc/rfc7617
        //   who needs passwords when we have keys?
        // - OAuth2.0 Authentication https://www.rfc-editor.org/rfc/rfc6749
        //   We should explore how OAuth flow can be mapped on Pubky statck.
        // - Digest Authentication https://www.rfc-editor.org/rfc/rfc7616
        //   This is basically how current auth with session works, check if this is a good start
        // - Client Certificates https://www.rfc-editor.org/rfc/rfc8705
        //   This is the way to go in the long run
        // Current PubkyAuth can be found here https://github.com/pubky/pubky-core/blob/main/docs/src/spec/auth.md
        .layer(AuthorizationLayer::new(state.clone()))
}
