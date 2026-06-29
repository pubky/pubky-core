//! Admin-only event stream (SSE): mirrors the client `/events-stream` but, gated by the admin
//! password, streams **all** events including private (`/priv/...`) paths. `user=` is an optional
//! filter (omit for every user); a single global `cursor` paginates. Response is `no-store`.

use std::convert::Infallible;

use axum::{
    extract::{RawQuery, State},
    http::{header, HeaderValue},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use futures_util::StreamExt;
use pubky_common::crypto::PublicKey;
use url::form_urlencoded;

use super::super::app_state::AppState;
use crate::{
    persistence::{
        files::events::{AllEventsFilter, PathFilter, MAX_EVENT_STREAM_USERS},
        sql::user::UserRepository,
    },
    shared::{webdav::WebDavPath, HttpError, HttpResult},
};

/// Parsed query parameters for the admin event stream.
struct AdminStreamParams {
    /// Optional user filter.
    users: Vec<PublicKey>,
    /// Optional starting cursor (global). None = from the beginning.
    cursor: Option<String>,
    /// Maximum total events to send before closing.
    limit: Option<u16>,
    /// Live streaming mode (cannot be combined with `reverse`).
    live: bool,
    /// Reverse (newest-first) ordering; batch-only.
    reverse: bool,
    /// Optional path filter. A trailing slash matches a directory and its descendants; without
    /// one it matches that exact file (see [`PathFilter`]).
    path: Option<WebDavPath>,
}

/// Parse the raw query string, handling repeated `user=` params.
///
/// `user=` is optional (absent = all users) and the cursor
/// is a single global `cursor=` value rather than a per-user `user=<pk>:<cursor>` suffix.
fn parse_admin_stream_query(query: &str) -> Result<AdminStreamParams, HttpError> {
    let mut users: Vec<PublicKey> = Vec::new();
    let mut cursor = None;
    let mut limit = None;
    let mut live = false;
    let mut reverse = false;
    let mut path: Option<String> = None;

    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "user" => {
                if value.is_empty() {
                    continue;
                }
                if PublicKey::is_pubky_prefixed(value.as_ref()) {
                    return Err(HttpError::bad_request(format!(
                        "Invalid public key: {value}"
                    )));
                }
                let pk = PublicKey::try_from_z32(value.as_ref())
                    .map_err(|_| HttpError::bad_request(format!("Invalid public key: {value}")))?;
                if !users.contains(&pk) {
                    users.push(pk);
                }
            }
            "cursor" => {
                if !value.is_empty() {
                    cursor = Some(value.to_string());
                }
            }
            "limit" => {
                limit = Some(
                    value
                        .parse::<u16>()
                        .map_err(|_| HttpError::bad_request(format!("Invalid limit: {value}")))?,
                );
            }
            "live" => live = value == "true" || value == "1",
            "reverse" => reverse = value == "true" || value == "1",
            "path" => {
                if !value.is_empty() {
                    path = Some(value.to_string());
                }
            }
            _ => {} // Ignore unknown parameters
        }
    }

    if live && reverse {
        return Err(HttpError::bad_request(
            "Cannot use live mode with reverse ordering",
        ));
    }
    if users.len() > MAX_EVENT_STREAM_USERS {
        return Err(HttpError::bad_request(format!(
            "Too many users. Maximum allowed: {MAX_EVENT_STREAM_USERS}"
        )));
    }

    let path = match path {
        Some(p) => {
            // Automatically prepend "/" if not present for caller convenience.
            let normalized = if p.starts_with('/') {
                p
            } else {
                format!("/{p}")
            };
            Some(
                WebDavPath::new(&normalized)
                    .map_err(|_| HttpError::bad_request(format!("Invalid path: {normalized}")))?,
            )
        }
        None => None,
    };

    Ok(AdminStreamParams {
        users,
        cursor,
        limit,
        live,
        reverse,
        path,
    })
}

/// Resolve parsed params into a service-layer [`AllEventsFilter`]: `404` for an unknown `user=`,
/// `400` for an invalid cursor. This is the route's only real work — the streaming itself lives in
/// [`EventsService::all_events_stream`].
async fn resolve_filter(
    state: &AppState,
    params: AdminStreamParams,
) -> HttpResult<AllEventsFilter> {
    // None = all users (firehose); otherwise resolve each pubkey to its id.
    let user_ids = if params.users.is_empty() {
        None
    } else {
        let mut ids = Vec::with_capacity(params.users.len());
        for pk in &params.users {
            let id = UserRepository::get_id(pk, &mut state.sql_db.pool().into())
                .await
                .map_err(|e| match e {
                    sqlx::Error::RowNotFound => HttpError::not_found(),
                    e => HttpError::from(e),
                })?;
            ids.push(id);
        }
        Some(ids)
    };

    let start_cursor = match params.cursor.as_deref() {
        Some(c) => Some(
            state
                .events_service
                .parse_cursor(c, &mut state.sql_db.pool().into())
                .await
                .map_err(|_| HttpError::bad_request("Invalid cursor"))?,
        ),
        None => None,
    };

    Ok(AllEventsFilter {
        start_cursor,
        user_ids,
        path: params.path.map(PathFilter::from),
        reverse: params.reverse,
        live: params.live,
        limit: params.limit,
    })
}

/// Admin-only SSE stream over **all** events (public and private). See [`AdminStreamParams`] for the
/// query parameters; the streaming lives in [`EventsService::all_events_stream`]. Response is
/// `text/event-stream`, `Cache-Control: no-store`.
pub async fn feed_stream(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> HttpResult<impl IntoResponse> {
    let params = parse_admin_stream_query(raw_query.as_deref().unwrap_or(""))?;
    let filter = resolve_filter(&state, params).await?;

    let sse = Sse::new(
        state
            .events_service
            .all_events_stream(state.sql_db.clone(), state.metrics.clone(), filter)
            .map(|event| {
                Ok::<_, Infallible>(
                    Event::default()
                        .event(event.event_type.to_string())
                        .data(event.to_sse_data()),
                )
            }),
    )
    .keep_alive(KeepAlive::default());

    // The feed surfaces private paths; never cache it.
    Ok((
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        sse,
    ))
}
