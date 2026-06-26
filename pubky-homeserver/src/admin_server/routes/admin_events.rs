//! Admin-only historical events feed.
//!
//! it exposes all events — public (`/pub/...`) and private (`/priv/...`) alike — for operational tooling,
//! debugging, and maintenance.
//!
//! The public feed (/events) is public-only and never leaks private paths, this endpoint is the one
//! place private paths are surfaced over the events feed, hence the admin auth requirement
//! and the `Cache-Control: no-store` header.

use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use super::super::app_state::AppState;
use crate::{
    client_server::{query_params::ListQueryParams, routes::events::format_events_feed},
    shared::{HttpError, HttpResult},
};

/// Admin-only historical events feed returning **all** events (public and private).
///
/// ## Query Parameters
/// - `cursor` (optional): Starting cursor position. Default: `"0"` (beginning).
/// - `limit` (optional): Maximum number of events to return.
///
/// ## Response Format
/// Identical to the public `GET /events/` feed — plain text, one line per event,
/// followed by the next cursor:
/// ```text
/// PUT pubky://user_pubkey/pub/example.txt
/// PUT pubky://user_pubkey/priv/app/secret.txt
/// DEL pubky://user_pubkey/pub/old.txt
/// cursor: 12345
/// ```
pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {
    // Treat a missing or empty cursor as "from the beginning", matching the public feed.
    let cursor_str = params.cursor.unwrap_or_else(|| "0".to_string());

    let cursor = state
        .events_service
        .parse_cursor(cursor_str.as_str(), &mut state.sql_db.pool().into())
        .await
        .map_err(|_| HttpError::bad_request("Invalid cursor"))?;

    let events = state
        .events_service
        .get_all_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
        .await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        // This feed exposes private paths; never let it be cached.
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(format_events_feed(&events)))
        .unwrap())
}
