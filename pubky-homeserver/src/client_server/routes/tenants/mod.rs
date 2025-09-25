//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{extract::DefaultBodyLimit, routing::any, routing::get, Router};

use crate::client_server::{
    layers::authz::AuthorizationLayer, layers::pubky_host::PubkyHostLayer, AppState,
};

use crate::shared::HttpResult;
use axum::{
    body::Body,
    extract::{Request, State},
    response::IntoResponse,
};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/session", get(session::session).delete(session::signout))
        .route("/dav/{key}/{*path}", any(dav_handler))
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

/// Dav path example:
/// https://qtnyghnq9swketdtj9drc7rs5pfnxhs61gq4jwd317ezdegcrbco/dav/qtnyghnq9swketdtj9drc7rs5pfnxhs61gq4jwd317ezdegcrbco/pub/test.txt
/// via https://github.com/pubky/pubky-core/pull/145#discussion_r2149297326

pub async fn dav_handler(
    State(state): State<AppState>,
    // Path((key, path)): Path<(Z32Pubkey, String)>,
    req: Request<Body>,
) -> HttpResult<impl IntoResponse> {
    let dav_response = state.inner_dav_handler.handle(req).await;
    Ok(dav_response.into_response())
}
