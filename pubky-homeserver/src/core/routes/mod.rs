//! The controller part of the [crate::HomeserverCore]

use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::core::AppState;

use super::layers::pubky_host::PubkyHostLayer;

mod auth;
mod feed;
mod public;
mod root;

fn base() -> Router<AppState> {
    Router::new()
        .route("/", get(root::handler))
        .route("/signup", post(auth::signup))
        .route("/session", post(auth::signin))
        // Routes for Pubky in the Hostname.
        //
        // The default and wortks with native Pubky client.
        // - Session routes
        .route("/session", get(auth::session))
        .route("/session", delete(auth::signout))
        // - Data routes
        // Events
        .route("/events/", get(feed::feed))
        .layer(CookieManagerLayer::new())
    // TODO: add size limit
    // TODO: revisit if we enable streaming big payloads
    // TODO: maybe add to a separate router (drive router?).
}

pub fn create_app(state: AppState) -> Router {
    base()
        .merge(public::data_store_router(state.clone()))
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
        .layer(PubkyHostLayer)
        .with_state(state)
}
