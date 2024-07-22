use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_cookies::CookieManagerLayer;
use tower_http::trace::TraceLayer;

use crate::server::AppState;

mod auth;
mod drive;
mod root;

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(root::handler))
        .route("/:pubky", put(auth::signup))
        .route("/:pubky/session", get(auth::session))
        .route("/:pubky/session", post(auth::signin))
        .route("/:pubky/session", delete(auth::signout))
        .route("/:pubky/*key", get(drive::put))
        .layer(TraceLayer::new_for_http())
        .layer(CookieManagerLayer::new())
        .with_state(state)
}
