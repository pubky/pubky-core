use axum::{
    body::Body,
    extract::{RawQuery, State},
    http::{header, Response, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use futures_util::stream::Stream;
use pubky_common::crypto::PublicKey;
use serde::Deserialize;
use std::{collections::HashMap, convert::Infallible, time::Instant};
use url::form_urlencoded;

use crate::{
    client_server::{extractors::ListQueryParams, AppState},
    metrics_server::routes::metrics::Metrics,
    persistence::{
        files::events::{EventCursor, EventEntity, EventsService, MAX_EVENT_STREAM_USERS},
        sql::SqlDb,
    },
    shared::{webdav::WebDavPath, HttpError, HttpResult},
};

#[derive(Debug, thiserror::Error)]
pub enum EventStreamError {
    #[error("User not found")]
    UserNotFound,
    #[error("{0}")]
    InvalidParameter(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
}

impl From<EventStreamError> for HttpError {
    fn from(error: EventStreamError) -> Self {
        match error {
            EventStreamError::UserNotFound => HttpError::not_found(),
            EventStreamError::DatabaseError(e) => HttpError::from(e),
            _ => HttpError::bad_request(error.to_string()),
        }
    }
}

/// Query parameters for the event stream SSE endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawEventStreamQueryParams")]
pub struct EventStreamQueryParams {
    /// Maximum total events to send before closing connection.
    pub limit: Option<u16>,
    /// Return events in reverse chronological order
    /// **Cannot be combined with `live=true`** (returns 400 error).
    pub reverse: bool,
    /// Enable live streaming mode
    pub live: bool,
    /// One or more user public keys to filter events for.
    /// - Format: z-base-32 encoded public key (e.g., "o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")
    /// - Single user: `?user=pubkey1`
    /// - Single user with cursor: `?user=pubkey1:cursor`
    /// - Multiple users: `?user=pubkey1&user=pubkey2:cursor2`
    pub user_cursors: Vec<(PublicKey, Option<String>)>,
    /// Path prefix to filter events.
    /// Format: Path WITHOUT `pubky://` scheme or user pubkey. Eg: `/pub/files/`, `pub/files/`, `/pub/`
    pub path: Option<WebDavPath>,
}

#[derive(Debug, Deserialize)]
struct RawEventStreamQueryParams {
    #[serde(default)]
    user: Vec<String>,
    limit: Option<u16>,
    #[serde(default)]
    reverse: bool,
    #[serde(default)]
    live: bool,
    path: Option<String>,
}

/// Parse query string manually to handle repeated `user` parameters.
/// URL query string format like: `user=pubkey1&user=pubkey2:cursor&limit=10&live=true`
fn parse_query_params(query: &str) -> Result<EventStreamQueryParams, EventStreamError> {
    let mut users = Vec::new();
    let mut limit = None;
    let mut reverse = false;
    let mut live = false;
    let mut path = None;

    // Parse using form_urlencoded which handles URL decoding
    for (key, value) in form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "user" => users.push(value.to_string()),
            "limit" => {
                limit = Some(value.parse::<u16>().map_err(|_| {
                    EventStreamError::InvalidParameter(format!("Invalid limit: {}", value))
                })?);
            }
            "reverse" => {
                reverse = value == "true" || value == "1";
            }
            "live" => {
                live = value == "true" || value == "1";
            }
            "path" => {
                if !value.is_empty() {
                    path = Some(value.to_string());
                }
            }
            _ => {} // Ignore unknown parameters
        }
    }

    let raw = RawEventStreamQueryParams {
        user: users,
        limit,
        reverse,
        live,
        path,
    };

    raw.try_into()
}

impl TryFrom<RawEventStreamQueryParams> for EventStreamQueryParams {
    type Error = EventStreamError;

    fn try_from(raw: RawEventStreamQueryParams) -> Result<Self, Self::Error> {
        if raw.live && raw.reverse {
            return Err(EventStreamError::InvalidParameter(
                "Cannot use live mode with reverse ordering".to_string(),
            ));
        }

        // Parse user values into (pubkey, optional_cursor) pairs
        // Format: "pubkey" or "pubkey:cursor"
        let mut user_cursors = Vec::new();
        for value in raw.user {
            if value.is_empty() {
                continue;
            }

            let (pubkey_str, cursor_str) = if let Some((pubkey, cursor)) = value.split_once(':') {
                (pubkey, Some(cursor))
            } else {
                (value.as_str(), None)
            };

            if PublicKey::is_pubky_prefixed(pubkey_str) {
                return Err(EventStreamError::InvalidPublicKey(pubkey_str.to_string()));
            }
            let pubkey = PublicKey::try_from_z32(pubkey_str)
                .map_err(|_| EventStreamError::InvalidPublicKey(pubkey_str.to_string()))?;

            user_cursors.push((pubkey, cursor_str.map(|s| s.to_string())));
        }

        if user_cursors.is_empty() {
            return Err(EventStreamError::InvalidParameter(
                "user parameter is required".to_string(),
            ));
        }

        if user_cursors.len() > MAX_EVENT_STREAM_USERS {
            return Err(EventStreamError::InvalidParameter(format!(
                "Too many users. Maximum allowed: {}",
                MAX_EVENT_STREAM_USERS
            )));
        }

        let path = if let Some(p) = raw.path {
            if p.is_empty() {
                None
            } else {
                // Automatically prepend "/" if not present for user convenience
                let normalized_path = if p.starts_with('/') {
                    p
                } else {
                    format!("/{}", p)
                };

                Some(WebDavPath::new(&normalized_path).map_err(|_| {
                    EventStreamError::InvalidParameter(format!("Invalid path: {}", normalized_path))
                })?)
            }
        } else {
            None
        };

        Ok(EventStreamQueryParams {
            limit: raw.limit,
            reverse: raw.reverse,
            live: raw.live,
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
/// data: content_hash: r0NJufX5oaagQE3qNtzJSZvLJcmtwRK3zJqTyuQfMmI= (only for PUT events, base64-encoded blake3 hash)
/// ```
fn formatted_event_path(entity: &EventEntity) -> String {
    // TODO: switch this formatter to use the shared `PubkyResource` type from `pubky-sdk`
    // once the homeserver crate depends on it directly, so we avoid ad-hoc string
    // reconstruction here.
    format!("pubky://{}{}", entity.user_pubkey.z32(), entity.path.path())
}

fn event_to_sse_data(entity: &EventEntity) -> String {
    let path = formatted_event_path(entity);
    let cursor_line = format!("cursor: {}", entity.cursor());

    let mut lines = vec![path, cursor_line];
    if let Some(hash) = entity.event_type.content_hash() {
        let hash_base64 =
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, hash.as_bytes());
        lines.push(format!("content_hash: {}", hash_base64));
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

    let query_start = Instant::now();
    let events = state
        .events_service
        .get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
        .await?;
    state
        .metrics
        .record_events_db_query(query_start.elapsed().as_millis());

    let mut result = events
        .iter()
        .map(|event| format!("{} {}", event.event_type, formatted_event_path(event)))
        .collect::<Vec<String>>();
    let next_cursor = events.last().map(|event| event.id.to_string());

    if let Some(next_cursor) = next_cursor {
        result.push(format!("cursor: {}", next_cursor));
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}

/// Server-Sent Events (SSE) endpoint for real-time event streaming.
///
/// Supports two modes:
/// - **Batch Mode** (`live=false` or omitted): Fetches historical events then closes connection
/// - **Streaming Mode** (`live=true`): Fetches historical events then streams new events in real-time
///
/// ## Slow Client Behavior
/// If a client cannot consume events fast enough in live mode, the broadcast channel will lag and the connection will be closed.
/// It is recommended that low memory clients poll this endpoint: Ie `live=true` with a low `limit`
///
/// ## Response Format
/// Each event is sent as an SSE message with the event type and multiline data:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 42
/// data: content_hash: r0NJufX5oaagQE3qNtzJSZvLJcmtwRK3zJqTyuQfMmI=
/// ```
pub async fn feed_stream(
    State(state): State<AppState>,
    raw_query: RawQuery,
) -> HttpResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let params =
        parse_query_params(raw_query.0.as_deref().unwrap_or("")).map_err(HttpError::from)?;
    let mut user_cursor_map =
        resolve_user_cursors(&params.user_cursors, &state.events_service, &state.sql_db)
            .await
            .map_err(HttpError::from)?;

    let mut total_sent: usize = 0;
    let stream = async_stream::stream! {
         // Create guard to ensure cleanup on any exit path (increments on creation, decrements on drop)
        let _guard = ConnectionGuard::new(state.metrics.clone());

        // Subscribe to broadcast channel immediately to prevent race condition
        // Events that occur during Phase 1 will be buffered in the channel
        let mut rx = state.events_service.subscribe();

        // Phase 1: Batch Mode
        loop {
            // Drain any buffered events before querying as they'll be included in this or a future database query
            while rx.try_recv().is_ok() {}

            let current_user_cursors: Vec<(i32, Option<EventCursor>)> =
                user_cursor_map.iter().map(|(k, cursor)| (*k, *cursor)).collect();

            let query_start = Instant::now();
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
            state.metrics.record_event_stream_db_query(query_start.elapsed().as_millis());

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
                        return;
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

        // Phase 2: Live mode
        if params.live {
            let user_ids: Vec<i32> = user_cursor_map.keys().copied().collect();
            let half_capacity = state.events_service.channel_capacity() / 2;

            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Check if receiver queue is at half capacity (early warning of slow clients)
                        if rx.len() >= half_capacity {
                            state.metrics.record_broadcast_half_full();
                        }
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
                                return;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        state.metrics.record_broadcast_lagged();
                        tracing::warn!(
                            "Slow client detected: broadcast channel lagged by {} events. Closing connection.",
                            skipped
                        );
                        return;
                    }
                    Err(_) => break, // Channel closed
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Resolve user public keys to user IDs and parse their cursors.
/// Returns a map of user_id → optional cursor position.
async fn resolve_user_cursors(
    user_cursors: &[(PublicKey, Option<String>)],
    events_service: &EventsService,
    sql_db: &SqlDb,
) -> Result<HashMap<i32, Option<EventCursor>>, EventStreamError> {
    use crate::persistence::sql::user::UserRepository;

    let mut user_cursor_map: HashMap<i32, Option<EventCursor>> = HashMap::new();

    for (user_pubkey, cursor_str_opt) in user_cursors {
        let user_id = UserRepository::get_id(user_pubkey, &mut sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => EventStreamError::UserNotFound,
                e => EventStreamError::DatabaseError(e),
            })?;

        let cursor = if let Some(cursor_str) = cursor_str_opt {
            Some(
                events_service
                    .parse_cursor(cursor_str, &mut sql_db.pool().into())
                    .await
                    .map_err(|_| {
                        EventStreamError::InvalidParameter(format!(
                            "Invalid cursor: {}",
                            cursor_str
                        ))
                    })?,
            )
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
    user_cursor_map: &HashMap<i32, Option<EventCursor>>,
    path_filter: Option<&WebDavPath>,
) -> bool {
    if !user_ids.contains(&event.user_id) {
        return false;
    }

    // Filter out events we already sent in Phase 1
    if let Some(Some(cursor)) = user_cursor_map.get(&event.user_id) {
        if event.cursor() <= *cursor {
            return false;
        }
    }

    if let Some(path) = path_filter {
        let path_suffix = event.path.path().as_str();
        if !path_suffix.starts_with(path.as_str()) {
            return false;
        }
    }

    true
}

/// Guard to ensure connection cleanup on any exit path
struct ConnectionGuard {
    metrics: Metrics,
    start: Instant,
}

impl ConnectionGuard {
    fn new(metrics: Metrics) -> Self {
        metrics.increment_active_connections();
        Self {
            metrics,
            start: Instant::now(),
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.metrics.decrement_active_connections();
        self.metrics
            .record_connection_closed(self.start.elapsed().as_millis());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_guard_drops_on_early_return() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Create guard and return early - guard should still decrement
        fn early_return_fn(metrics: Metrics) -> Result<(), &'static str> {
            let _guard = ConnectionGuard::new(metrics.clone());
            // Simulate early return (e.g., error condition)
            return Err("early exit");
            #[allow(unreachable_code)]
            {
                Ok(())
            }
        }

        let result = early_return_fn(metrics.clone());
        assert!(result.is_err(), "Should have returned early");

        // Verify guard cleaned up properly despite early return
        let output = metrics.render().expect("Failed to render metrics");
        assert!(
            output.contains("event_stream_active_connections") && output.contains("} 0"),
            "Should have 0 active connections after early return: {}",
            output
        );
        assert!(
            output.contains("event_stream_connection_duration_ms_count"),
            "Should have recorded connection duration: {}",
            output
        );
    }

    #[tokio::test]
    async fn connection_guard_concurrent() {
        let metrics = Metrics::new().expect("Failed to create metrics");

        // Create 5 concurrent guards using tokio::spawn
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let metrics_clone = metrics.clone();
                tokio::spawn(async move {
                    let _guard = ConnectionGuard::new(metrics_clone);
                    // Simulate some work
                    tokio::time::sleep(tokio::time::Duration::from_millis(10 * i)).await;
                    // Guard will be dropped here
                })
            })
            .collect();

        // While tasks are running, check active connections
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let output = metrics.render().expect("Failed to render metrics");
        // We should have some active connections (implementation dependent on timing)
        assert!(
            output.contains("event_stream_active_connections"),
            "Should have active connections metric: {}",
            output
        );

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // All guards should be cleaned up
        let output = metrics.render().expect("Failed to render metrics");
        assert!(
            output.contains("event_stream_active_connections") && output.contains("} 0"),
            "Should have 0 active connections after all concurrent guards dropped: {}",
            output
        );
        assert!(
            output.contains("event_stream_connection_duration_ms_count") && output.contains("} 5"),
            "Should have recorded 5 connection durations: {}",
            output
        );
    }
}
