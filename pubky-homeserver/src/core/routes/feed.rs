use axum::{
    body::Body,
    extract::{FromRequestParts, Query, State},
    http::{header, request::Parts, Response, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    RequestPartsExt,
};
use futures_util::stream::Stream;
use pkarr::PublicKey;
use std::{collections::HashMap, convert::Infallible};

use crate::{
    core::{extractors::ListQueryParams, AppState},
    persistence::{
        files::events::{Cursor, EventEntity, EventsService, MAX_EVENT_STREAM_USERS},
        sql::SqlDb,
    },
    shared::{parse_bool, webdav::WebDavPath, HttpError, HttpResult},
};

#[derive(Debug, Clone)]
pub struct EventStreamQueryParams {
    pub limit: Option<u16>,
    pub reverse: bool,
    /// If true, enter live streaming mode after historical events.
    /// If false, close connection after historical events are exhausted.
    pub live: bool,
    /// Vec of (user_pubkey, optional_cursor) pairs
    /// Format: "user=pubkey" or "user=pubkey:cursor"
    pub user_cursors: Vec<(PublicKey, Option<String>)>,
    /// Optional path prefix filter
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "pub/files/")
    ///   - Example: `path=/pub/` will only return events under the `/pub/` directory
    ///   - Example: `path=/pub/files/` will only return events under the `/pub/files/` directory
    pub path: Option<WebDavPath>,
}

impl<S> FromRequestParts<S> for EventStreamQueryParams
where
    S: Send + Sync,
{
    type Rejection = Response<Body>;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let params: Query<HashMap<String, String>> =
            parts.extract().await.map_err(IntoResponse::into_response)?;

        // Manually parse the "user" parameter since it can appear multiple times
        let query = parts.uri.query().unwrap_or("");
        let mut user_values: Vec<String> = Vec::new();
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if key.as_ref() == "user" && !value.is_empty() {
                user_values.push(value.into_owned());
            }
        }

        let reverse = if let Some(reverse_str) = params.get("reverse") {
            parse_bool(reverse_str).map_err(|e| *e)?
        } else {
            false
        };

        let live = if let Some(live_str) = params.get("live") {
            parse_bool(live_str).map_err(|e| *e)?
        } else {
            false
        };

        let limit = match params.get("limit") {
            Some(l) if !l.is_empty() => l.parse::<u16>().ok(),
            _ => None,
        };

        let path = params.get("path").and_then(|p| {
            if p.is_empty() {
                return None;
            }

            // Automatically prepend "/" if not present for user convenience
            let normalized_path = if p.starts_with('/') {
                p.clone()
            } else {
                format!("/{}", p)
            };

            WebDavPath::new(&normalized_path).ok() // Invalid path, treat as if no path was provided
        });

        // Parse user values into (pubkey, optional_cursor) pairs
        // Format: "pubkey" or "pubkey:cursor"
        let mut user_cursors = Vec::new();
        for value in user_values {
            let (pubkey_str, cursor_str) = if let Some((pubkey, cursor)) = value.split_once(':') {
                (pubkey, Some(cursor))
            } else {
                (value.as_str(), None)
            };

            let pubkey = match PublicKey::try_from(pubkey_str) {
                Ok(pk) => pk,
                Err(_) => {
                    return Err(Response::builder()
                        .status(StatusCode::BAD_REQUEST)
                        .body(Body::from(format!("Invalid public key: {}", pubkey_str)))
                        .unwrap()
                        .into_response());
                }
            };

            user_cursors.push((pubkey, cursor_str.map(|s| s.to_string())));
        }

        Ok(EventStreamQueryParams {
            limit,
            reverse,
            live,
            user_cursors,
            path,
        })
    }
}

/// Format an event entity as SSE event data.
/// Returns the multiline data field content.
/// Each line will be prefixed with "data: " by the SSE library.
///
/// ## Format
/// ```text
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: abc123... (optional, only if present)
/// ```
fn event_to_sse_data(entity: &EventEntity) -> String {
    let path = format!("pubky://{}", entity.path.as_str());
    let cursor_line = format!("cursor: {}", entity.cursor());

    let mut lines = vec![path, cursor_line];
    if let Some(hash) = entity.content_hash {
        lines.push(format!("content_hash: {}", hash.to_hex()));
    }
    lines.join("\n")
}

/// Legacy text-based endpoint for fetching historical events.
///
/// ## Query Parameters
/// - `cursor` (optional): Starting cursor position. Default: "0" (beginning)
/// - `limit` (optional): Maximum number of events to return
///
/// ## Response Format
/// Plain text response with one line per event, followed by the next cursor:
/// ```text
/// PUT pubky://user_pubkey/pub/example.txt
/// DEL pubky://user_pubkey/pub/old.txt
/// PUT pubky://user_pubkey/pub/another.txt
/// cursor: 12345
/// ```
pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {
    let cursor = match params.cursor {
        Some(cursor) => cursor,
        None => "0".to_string(),
    };

    let cursor = match state
        .events_service
        .parse_cursor(cursor.as_str(), &mut state.sql_db.pool().into())
        .await
    {
        Ok(cursor) => cursor,
        Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
    };

    let events = state
        .events_service
        .get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
        .await?;
    let mut result = events
        .iter()
        .map(|event| format!("{} pubky://{}", event.event_type, event.path.as_str()))
        .collect::<Vec<String>>();
    let next_cursor = events
        .last()
        .map(|event| event.id.to_string())
        .unwrap_or("".to_string());
    result.push(format!("cursor: {}", next_cursor));

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}

/// Server-Sent Events (SSE) endpoint for real-time event streaming.
///
/// This endpoint supports two modes of operation:
///
/// ## Batch Mode (`live=false` or omitted)
/// - Fetches historical events from the cursor onwards
/// - Streams events progressively as they're fetched
/// - **Closes connection** when all historical events are sent (or `limit` reached)
/// - Use case: Traditional GET request/response pattern for fetching historical events
///
/// ## Streaming Mode (`live=true`)
/// - **Phase 1**: Fetches historical events from cursor onwards (same as batch mode)
/// - **Phase 2**: Subscribes to broadcast channel for new events
/// - Streams new events in real-time as they occur
/// - Connection stays open indefinitely
///
/// ## Query Parameters
/// - `user` (**REQUIRED**): One or more user public keys to filter events for.
///   - Format: z-base-32 encoded public key (e.g., "o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")
///   - Single user: `?user=pubkey1`
///   - Single user with cursor: `?user=pubkey1:cursor`
///   - Multiple users with and without cursor: `?user=pubkey1&user=pubkey2:cursor2`
///   - Maximum: 50 users per request
/// - `live` (optional): Enable live streaming mode. Default: `false` (batch mode)
///   - `live=false` or omitted: Fetch historical events and close connection
///   - `live=true`: Fetch historical events, then stream new events in real-time
///   - **Cannot be combined with `reverse=true`** (will return 400 error)
/// - `limit` (optional): Maximum total events to send before closing connection.
///   - If **omitted with `live=false`**: Send all historical events, then close
///   - If **omitted with `live=true`**: Send all historical events, then enter live mode (infinite stream)
///   - If **specified**: Send up to N events total, then close connection (regardless of `live` setting)
/// - `reverse` (optional): Return events in reverse chronological order (newest first).
///   Default: `false` (oldest first)
///   - When `true`, only historical events are returned (batch mode enforced)
///   - **Cannot be combined with `live=true`** (will return 400 error)
/// - `path` (optional): Path prefix to filter events. Only events whose path starts with this prefix are returned.
///   - Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "pub/files/")
///
/// ## SSE Response Format
/// Each event is sent as an SSE message with the event type and multiline data:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
/// ```
pub async fn feed_stream(
    State(state): State<AppState>,
    params: EventStreamQueryParams,
) -> HttpResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    // Validate parameters
    validate_stream_params(&params)?;

    // Resolve user IDs and cursors
    let mut user_cursor_map =
        resolve_user_cursors(&params.user_cursors, &state.events_service, &state.sql_db).await?;

    let mut total_sent: usize = 0;
    let stream = async_stream::stream! {
        // Subscribe to broadcast channel immediately to prevent race condition
        // Events that occur during Phase 1 will be buffered in the channel
        let mut rx = state.events_service.subscribe();

        // Phase 1: Historical auto-pagination
        // Fetch all historical events in batches until caught up
        loop {
            // Drain any buffered events before querying as they'll be included in this or a future database query
            while rx.try_recv().is_ok() {}

            let current_user_cursors: Vec<(i32, Option<Cursor>)> =
                user_cursor_map.iter().map(|(k, cursor)| (*k, *cursor)).collect();

            let events = match state
                .events_service
                .get_by_user_cursors(
                    current_user_cursors,
                    params.reverse,
                    params.path.as_ref().map(|p| p.as_str()),
                    &mut state.sql_db.pool().into(),
                )
                .await
            {
                Ok(events) => events,
                Err(e) => {
                    tracing::error!("Database error while fetching events: {}", e);
                    break;
                }
            };

            let event_count = events.len();

            // Stream each historical event
            for event in events {
                // Update the cursor for this specific user
                user_cursor_map.insert(event.user_id, Some(event.cursor()));

                yield Ok(Event::default()
                    .event(event.event_type.to_string())
                    .data(event_to_sse_data(&event)));

                total_sent += 1;

                // Close if we've reached limit
                if let Some(max) = params.limit {
                    if total_sent >= max as usize {
                        return; // Close connection
                    }
                }
            }

            // If we got zero events, all users are caught up with history
            if event_count == 0 {
                // If not in live mode, close connection (batch mode)
                if !params.live {
                    return;
                }
                // Otherwise, transition to live mode
                break;
            }

            // If we got a partial batch (> 0 but less than read_batch_size), continue querying
            // Some users might still have more events even though this batch was partial
        }

        // Phase 2: Live mode - stream new events from broadcast channel
        if params.live {

            // Extract user_ids for filtering
            let user_ids: Vec<i32> = user_cursor_map.keys().copied().collect();

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Filter events based on user_ids, cursors, and path
                        if !should_include_live_event(&event, &user_ids, &user_cursor_map, params.path.as_ref()) {
                            continue;
                        }

                        // Update this user's cursor
                        user_cursor_map.insert(event.user_id, Some(event.cursor()));

                        yield Ok(Event::default()
                            .event(event.event_type.to_string())
                            .data(event_to_sse_data(&event)));

                        total_sent += 1;

                        // Close if we've reached limit
                        if let Some(max) = params.limit {
                            if total_sent >= max as usize {
                                return; // Close connection
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::error!(
                            "Broadcast channel lagged: {} events were skipped during historical query",
                            skipped
                        );
                        continue;
                    }
                    Err(_) => break, // Channel closed
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Validate event stream parameters before processing.
/// Returns error if parameters are invalid or incompatible.
fn validate_stream_params(params: &EventStreamQueryParams) -> HttpResult<()> {
    if params.user_cursors.is_empty() {
        return Err(HttpError::bad_request("user parameter is required"));
    }

    if params.user_cursors.len() > MAX_EVENT_STREAM_USERS {
        return Err(HttpError::bad_request(format!(
            "Too many users. Maximum allowed: {}",
            MAX_EVENT_STREAM_USERS
        )));
    }

    if params.live && params.reverse {
        return Err(HttpError::bad_request(
            "Cannot use live mode with reverse ordering",
        ));
    }

    Ok(())
}

/// Resolve user public keys to user IDs and parse their cursors.
/// Returns a map of user_id → optional cursor position.
async fn resolve_user_cursors(
    user_cursors: &[(PublicKey, Option<String>)],
    events_service: &EventsService,
    sql_db: &SqlDb,
) -> HttpResult<HashMap<i32, Option<Cursor>>> {
    use crate::persistence::sql::user::UserRepository;

    let mut user_cursor_map: HashMap<i32, Option<Cursor>> = HashMap::new();

    for (user_pubkey, cursor_str_opt) in user_cursors {
        let user_id = UserRepository::get_id(user_pubkey, &mut sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => HttpError::not_found(),
                _ => HttpError::from(e),
            })?;

        let cursor = if let Some(cursor_str) = cursor_str_opt {
            match events_service
                .parse_cursor(cursor_str, &mut sql_db.pool().into())
                .await
            {
                Ok(cursor) => Some(cursor),
                Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
            }
        } else {
            None
        };

        user_cursor_map.insert(user_id, cursor);
    }

    Ok(user_cursor_map)
}

/// Filter events in live mode based on user IDs, cursors, and path prefix.
/// Returns true if the event should be included in the stream.
fn should_include_live_event(
    event: &EventEntity,
    user_ids: &[i32],
    user_cursor_map: &HashMap<i32, Option<Cursor>>,
    path_filter: Option<&WebDavPath>,
) -> bool {
    // Filter by user_ids
    if !user_ids.contains(&event.user_id) {
        return false;
    }

    // Filter out events we already sent in Phase 1
    if let Some(Some(cursor)) = user_cursor_map.get(&event.user_id) {
        if event.cursor() <= *cursor {
            return false; // Already sent this event
        }
    }

    // Filter by path prefix if specified
    if let Some(path) = path_filter {
        let path_suffix = event.path.path().as_str();
        if !path_suffix.starts_with(path.as_str()) {
            return false;
        }
    }

    true
}
