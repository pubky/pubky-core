//! Per Tenant (user / Pubky) routes.
//!
//! Every route here is relative to a tenant's Pubky host,
//! as opposed to routes relative to the Homeserver's owner.

use axum::{extract::DefaultBodyLimit, routing::get, Router};

use crate::client_server::{layers::authz::AuthorizationLayer, AppState};

pub mod read;
pub mod session;
pub mod write;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/session", get(session::session).delete(session::signout))
        // XXX: dzdidi new path example:
        // https://qtnyghnq9swketdtj9drc7rs5pfnxhs61gq4jwd317ezdegcrbco/dav/qtnyghnq9swketdtj9drc7rs5pfnxhs61gq4jwd317ezdegcrbco/pub/test.txt
        // via https://github.com/pubky/pubky-core/pull/145#discussion_r2149297326
        .route(
            "/{*path}",
            get(read::get)
                .head(read::head)
                .put(write::put)
                .delete(write::delete),
        )
        // TODO: different max size for sessions and other routes?
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        // XXX: dzdidi - WebDAV compliant auth. Which is actually http basic auth so we need some magic here
        // to make session based auth look like http auth while also accepting http basic auth for webDAV comp
        .layer(AuthorizationLayer::new(state.clone()))
}
