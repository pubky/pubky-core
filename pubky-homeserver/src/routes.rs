use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::server::AppState;

use self::pkarr::pkarr_router;

mod auth;
mod feed;
mod pkarr;
mod public;
mod root;

fn base(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::handler))
        .route("/signup", post(auth::signup))
        .route("/session", post(auth::signin))
        .route("/:pubky/session", get(auth::session))
        .route("/:pubky/session", delete(auth::signout))
        .route("/:pubky/*path", put(public::put))
        .route("/:pubky/*path", get(public::get))
        .route("/:pubky/*path", delete(public::delete))
        .route("/events/", get(feed::feed))
        .layer(CookieManagerLayer::new())
        // TODO: revisit if we enable streaming big payloads
        // TODO: maybe add to a separate router (drive router?).
        .layer(DefaultBodyLimit::max(16 * 1024))
        .with_state(state)
}

pub fn create_app(state: AppState) -> Router {
    base(state.clone())
        // TODO: Only enable this for test environments?
        .nest("/pkarr", pkarr_router(state))
        .layer(CorsLayer::very_permissive())
        .layer(TraceLayer::new_for_http())
}
