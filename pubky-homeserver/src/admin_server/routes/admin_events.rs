//! Admin-only event stream (SSE): mirrors the client `/events-stream` but, gated by the admin
//! password, streams **all** events including private (`/priv/...`) paths. `user=` is an optional
//! filter (omit for every user); a single global `cursor` paginates. Response is `no-store`.

use std::{convert::Infallible, time::Instant};

use axum::{
    extract::{RawQuery, State},
    http::{header, HeaderValue},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use pubky_common::crypto::PublicKey;
use url::form_urlencoded;

use super::super::app_state::AppState;
use crate::{
    metrics_server::routes::metrics::ConnectionGuard,
    persistence::{
        files::events::{EventCursor, EventEntity, MAX_EVENT_STREAM_USERS},
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
    /// Optional path-prefix filter (literal prefix — use a trailing slash to scope to a directory).
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

/// Decide whether a live (broadcast) event belongs in the stream.
fn admin_should_include_live_event(
    event: &EventEntity,
    last_cursor: Option<EventCursor>,
    user_ids: Option<&[i32]>,
    path_filter: Option<&WebDavPath>,
) -> bool {
    if let Some(cursor) = last_cursor {
        if event.cursor() <= cursor {
            return false;
        }
    }
    if let Some(ids) = user_ids {
        if !ids.contains(&event.user_id) {
            return false;
        }
    }
    if let Some(path) = path_filter {
        if !event.path.path().as_str().starts_with(path.as_str()) {
            return false;
        }
    }
    true
}

/// Admin-only SSE stream over **all** events (public and private). See [`AdminStreamParams`]
/// for the query parameters. Response is `text/event-stream`, `Cache-Control: no-store`, with the
/// same per-event framing as the client `/events-stream` (see [`EventEntity::to_sse_data`]).
pub async fn feed_stream(
    State(state): State<AppState>,
    RawQuery(raw_query): RawQuery,
) -> HttpResult<impl IntoResponse> {
    let params = parse_admin_stream_query(raw_query.as_deref().unwrap_or(""))?;

    // Resolve the optional user filter to user ids. None = all users (firehose).
    let user_ids: Option<Vec<i32>> = if params.users.is_empty() {
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

    // Parse the starting cursor (None = from the beginning).
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

    let reverse = params.reverse;
    let live = params.live;
    let limit = params.limit;
    let path = params.path;

    let mut total_sent: usize = 0;
    let mut last_cursor: Option<EventCursor> = start_cursor;

    let stream = async_stream::stream! {
        // Cleanup + connection metrics on any exit path.
        let _guard = ConnectionGuard::new(state.metrics.clone());

        // Subscribe up front so events that occur during Phase 1 are buffered, not missed.
        let mut rx = state.events_service.subscribe();

        // Phase 1: historical replay over a single advancing global cursor.
        loop {
            // Drain buffered events; they'll be covered by this or a later DB query.
            while rx.try_recv().is_ok() {}

            let query_start = Instant::now();
            let events = match state
                .events_service
                .get_all_events(
                    last_cursor,
                    None,
                    reverse,
                    path.as_ref().map(|p| p.as_str()),
                    user_ids.as_deref(),
                    &mut state.sql_db.pool().into(),
                )
                .await
            {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Database error while fetching admin events: {}", e);
                    break;
                }
            };
            state
                .metrics
                .record_event_stream_db_query(query_start.elapsed().as_millis());

            let event_count = events.len();
            for event in events {
                // Check the limit before yielding so `limit=0` sends nothing.
                if let Some(max) = limit {
                    if total_sent >= max as usize {
                        return;
                    }
                }
                last_cursor = Some(event.cursor());
                yield Ok::<_, Infallible>(
                    Event::default()
                        .event(event.event_type.to_string())
                        .data(event.to_sse_data()),
                );
                total_sent += 1;
            }

            // A query with no events means we've replayed everything up to `last_cursor`.
            if event_count == 0 {
                if !live {
                    return;
                }
                break;
            }
        }

        // Phase 2: live mode (ascending only; reverse+live is rejected at parse time).
        if live {
            let half_capacity = state.events_service.channel_capacity() / 2;
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if rx.len() >= half_capacity {
                            state.metrics.record_broadcast_half_full();
                        }
                        if !admin_should_include_live_event(
                            &event,
                            last_cursor,
                            user_ids.as_deref(),
                            path.as_ref(),
                        ) {
                            continue;
                        }
                        // Check the limit before yielding so `limit=0` sends nothing.
                        if let Some(max) = limit {
                            if total_sent >= max as usize {
                                return;
                            }
                        }
                        last_cursor = Some(event.cursor());
                        yield Ok::<_, Infallible>(
                            Event::default()
                                .event(event.event_type.to_string())
                                .data(event.to_sse_data()),
                        );
                        total_sent += 1;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        state.metrics.record_broadcast_lagged();
                        tracing::warn!(
                            "Slow admin client detected: broadcast channel lagged by {} events. Closing connection.",
                            skipped
                        );
                        return;
                    }
                    Err(_) => break, // Channel closed
                }
            }
        }
    };

    let sse = Sse::new(stream).keep_alive(KeepAlive::default());
    // The feed surfaces private paths; never cache it.
    Ok((
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        sse,
    ))
}
