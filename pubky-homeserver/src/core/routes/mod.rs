//! The controller part of the [super::HomeserverCore]

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use tower::ServiceBuilder;
use tower_cookies::CookieManagerLayer;
use tower_http::cors::CorsLayer;

use crate::{core::AppState, shared::PubkyHostLayer};

use super::layers::trace::with_trace_layer;

mod auth;
mod feed;
mod root;
mod tenants;

static HOMESERVER_VERSION: &str = concat!("pubky.org", "@", env!("CARGO_PKG_VERSION"),);
const TRACING_EXCLUDED_PATHS: [&str; 1] = ["/events/"];

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
    let app = base()
        .merge(tenants::router(state.clone()))
        .layer(CookieManagerLayer::new())
        .layer(CorsLayer::very_permissive())
        .layer(ServiceBuilder::new().layer(middleware::from_fn(add_server_header)))
        .with_state(state);

    // Apply trace and pubky host layers to the complete router.
    with_trace_layer(app, &TRACING_EXCLUDED_PATHS).layer(PubkyHostLayer)
}

// Middleware to add a `Server` header to all responses
async fn add_server_header(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;

    // Add a custom header to the response
    response
        .headers_mut()
        .insert(header::SERVER, HeaderValue::from_static(HOMESERVER_VERSION));

    response
}
