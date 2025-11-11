use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
};
use futures_util::stream::Stream;
use std::{collections::HashMap, convert::Infallible, time::Instant};

use crate::{
    core::{
        extractors::{EventStreamQueryParams, ListQueryParams},
        AppState, Metrics,
    },
    persistence::sql::event::{Cursor, EventRepository, EventResponse},
    shared::{HttpError, HttpResult},
};

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

    let cursor =
        match EventRepository::parse_cursor(cursor.as_str(), &mut state.sql_db.pool().into()).await
        {
            Ok(cursor) => cursor,
            Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
        };

    let query_start = Instant::now();
    let events =
        EventRepository::get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
            .await?;
    let elapsed_ms = query_start.elapsed().as_millis();

    let mut result = events
        .iter()
        .map(|event| format!("{} pubky://{}", event.event_type, event.path.as_str()))
        .collect::<Vec<String>>();
    let next_cursor = events
        .last()
        .map(|event| event.id.to_string())
        .unwrap_or("".to_string());
    result.push(format!("cursor: {}", next_cursor));

    state.metrics.record_events_db_query(elapsed_ms);
    tracing::debug!(
        elapsed_ms = elapsed_ms,
        events_returned = events.len(),
        "/events request completed"
    );

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
    use crate::constants::MAX_EVENT_STREAM_USERS;
    use crate::persistence::sql::user::UserRepository;
    use pkarr::PublicKey;
    use std::str::FromStr;

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

    // Parse all pubkeys, get user IDs, and parse cursors
    // Return Error if any keys or cursors invalid
    let mut user_cursor_map: HashMap<i32, Option<Cursor>> = HashMap::new();
    for (user_pubkey_str, cursor_str_opt) in &params.user_cursors {
        let user_pubkey = PublicKey::from_str(user_pubkey_str)
            .map_err(|_| HttpError::bad_request("Invalid user public key"))?;

        let user_id = UserRepository::get_id(&user_pubkey, &mut state.sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => HttpError::not_found(),
                _ => HttpError::from(e),
            })?;

        let cursor = if let Some(cursor_str) = cursor_str_opt {
            match EventRepository::parse_cursor(cursor_str, &mut state.sql_db.pool().into()).await {
                Ok(cursor) => Some(cursor),
                Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
            }
        } else {
            None
        };

        user_cursor_map.insert(user_id, cursor);
    }

    // Track connection start
    state.metrics.increment_active_connections();

    tracing::info!(
        user_count = user_cursor_map.len(),
        live_mode = params.live,
        has_path_filter = params.path.is_some(),
        "Event stream connection established"
    );

    let mut total_sent: usize = 0;
    let stream = async_stream::stream! {
        // Create guard to ensure cleanup on any exit path
        let _guard = ConnectionGuard::new(state.metrics.clone());

        // Subscribe to broadcast channel immediately to prevent race condition
        // Events that occur during Phase 1 will be buffered in the channel
        let mut rx = state.event_tx.subscribe();

        // Phase 1: Historical auto-pagination
        // Fetch all historical events in batches until caught up
        loop {
            // Drain any buffered events before querying as they'll be included in this or a future database query
            while rx.try_recv().is_ok() {}

            let current_user_cursors: Vec<(i32, Option<Cursor>)> =
                user_cursor_map.iter().map(|(k, cursor)| (*k, *cursor)).collect();

            // TIME THE QUERY
            let query_start = Instant::now();
            let events = match EventRepository::get_by_user_cursors(
                current_user_cursors,
                params.reverse,
                params.path.as_deref(),
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

            let elapsed_ms = query_start.elapsed().as_millis();
            state.metrics.record_event_stream_db_query(elapsed_ms);
            tracing::debug!(
                elapsed_ms = elapsed_ms,
                events_fetched = events.len(),
                user_count = user_cursor_map.len(),
                "Phase 1 batch query completed"
            );

            let event_count = events.len();

            // Stream each historical event
            for event in events {
                // Update the cursor for this specific user
                user_cursor_map.insert(event.user_id, Some(event.cursor()));

                let response = EventResponse::from_entity(&event);
                yield Ok(Event::default()
                    .event(response.event_type.to_string())
                    .data(response.to_sse_data()));

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
                        // Filter by user_ids
                        if !user_ids.contains(&event.user_id) {
                            continue;
                        }

                        // Filter out events we already sent in Phase 1
                        if let Some(Some(cursor)) = user_cursor_map.get(&event.user_id) {
                            if event.cursor() <= *cursor {
                                continue; // Already sent this event
                            }
                        }

                        // Filter by path prefix if specified
                        if let Some(ref path) = params.path {
                            let path_suffix = event.path.path().as_str();
                            if !path_suffix.starts_with(path) {
                                continue;
                            }
                        }

                        // Update this user's cursor
                        user_cursor_map.insert(event.user_id, Some(event.cursor()));

                        let response = EventResponse::from_entity(&event);
                        yield Ok(Event::default()
                            .event(response.event_type.to_string())
                            .data(response.to_sse_data()));

                        total_sent += 1;

                        // Close if we've reached limit
                        if let Some(max) = params.limit {
                            if total_sent >= max as usize {
                                return; // Close connection
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        state.metrics.record_broadcast_lagged();
                        tracing::warn!(
                            missed_events = skipped,
                            user_count = user_ids.len(),
                            "Event stream receiver lagged behind broadcast channel"
                        );
                        continue;
                    }
                    Err(_) => {
                        tracing::info!("Broadcast channel closed, ending stream");
                        break;
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Guard to ensure connection cleanup on any exit path
struct ConnectionGuard {
    metrics: Metrics,
    start: Instant,
}

impl ConnectionGuard {
    fn new(metrics: Metrics) -> Self {
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
            .record_connection_closed(self.start.elapsed().as_secs());

        tracing::info!(
            duration_s = self.start.elapsed().as_secs(),
            "Event stream connection closed"
        );
    }
}
