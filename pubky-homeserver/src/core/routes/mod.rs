//! The controller part of the [crate::HomeserverCore]

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, head, post, put},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::core::AppState;

mod auth;
mod feed;
mod public;
mod root;

fn base(state: AppState) -> Router {
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
        .route("/pub/", get(public::read::list_root))
        .route("/pub/*path", get(public::read::get))
        .route("/pub/*path", head(public::read::head))
        .route("/pub/*path", put(public::write::put))
        .route("/pub/*path", delete(public::write::delete))
        // Pubky in the path.
        //
        // Important to support web browsers until they support Pkarr domains natively.
        // - Session routes
        .route("/:pubky/session", get(auth::session))
        .route("/:pubky/session", delete(auth::signout))
        // - Data routes
        .route("/:pubky/*path", get(public::read::get))
        .route("/:pubky/*path", head(public::read::head))
        .route("/:pubky/*path", put(public::write::put))
        .route("/:pubky/*path", delete(public::write::delete))
        // Events
        .route("/events/", get(feed::feed))
        .layer(CookieManagerLayer::new())
        // TODO: revisit if we enable streaming big payloads
        // TODO: maybe add to a separate router (drive router?).
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .with_state(state)
}

pub fn create_app(state: AppState) -> Router {
    base(state.clone())
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
}
