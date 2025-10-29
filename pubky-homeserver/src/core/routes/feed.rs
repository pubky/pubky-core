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
use std::{collections::HashMap, convert::Infallible};

use crate::{
    core::{
        extractors::{EventStreamQueryParams, ListQueryParams},
        AppState,
    },
    persistence::sql::event::{EventCursor, EventRepository, EventResponse},
    shared::{HttpError, HttpResult},
};

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

    let events = EventRepository::get_by_cursor(
        None,
        Some(cursor),
        params.limit,
        false,
        &mut state.sql_db.pool().into(),
    )
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
/// - Queries the database in batches of 100 events (internal optimization)
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
/// - `user` (**REQUIRED**): One or more user public keys to filter events for. Can be repeated multiple times.
///   - Format: z-base-32 encoded public key (e.g., "o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")
///   - Single user: `?user=pubkey1`
///   - Multiple users: `?user=pubkey1&user=pubkey2&user=pubkey3`
///   - Per-user cursors: `?user=pubkey1:cursor1&user=pubkey2:cursor2`
///   - Maximum: 50 users per request
///   - The endpoint uses the `(user, created_at, id)` database index for efficient querying
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
/// - `filter_dir` (optional): Path prefix to filter events. Only events whose path starts with this prefix are returned.
///   - Format: Path WITHOUT `pubky://` scheme or user pubkey (e.g., "/pub/files/" or "/pub/")
///   - The prefix must start with "/" and is matched against the WebDAV path stored in the database
///   - Example: `filter_dir=/pub/` will only return events under the `/pub/` directory
///   - Example: `filter_dir=/pub/files/` will only return events under the `/pub/files/` directory
///
/// ## SSE Response Format
/// Each event is sent as an SSE message with the event type and multiline data:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 00331BD814YCT:42
/// data: content_hash: af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
/// ```
///
/// Or for DELETE events (no content_hash):
/// ```text
/// event: DEL
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 00331BD814YCT:43
/// ```
///
/// **Note**: The `content_hash` field is optional and only included for PUT events when the hash is available.
/// Legacy events created before the content_hash feature was added will not have this field.
pub async fn feed_stream(
    State(state): State<AppState>,
    params: EventStreamQueryParams,
) -> HttpResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    use crate::constants::MAX_EVENT_STREAM_USERS;
    use crate::persistence::sql::user::UserRepository;
    use pkarr::PublicKey;
    use std::str::FromStr;

    // User parameter is REQUIRED for events-stream
    if params.user_cursors.is_empty() {
        return Err(HttpError::bad_request("user parameter is required"));
    }

    // Validate max users
    if params.user_cursors.len() > MAX_EVENT_STREAM_USERS {
        return Err(HttpError::bad_request(format!(
            "Too many users. Maximum allowed: {}",
            MAX_EVENT_STREAM_USERS
        )));
    }

    // Validate incompatible parameter combinations
    if params.live && params.reverse {
        return Err(HttpError::bad_request(
            "Cannot use live mode with reverse ordering",
        ));
    }

    // Parse all pubkeys, get user IDs, and parse cursors
    let mut user_cursor_map: HashMap<i32, Option<EventCursor>> = HashMap::new();

    for (user_pubkey_str, cursor_str_opt) in &params.user_cursors {
        let user_pubkey = PublicKey::from_str(user_pubkey_str)
            .map_err(|_| HttpError::bad_request("Invalid user public key"))?;

        let user_id = UserRepository::get_id(&user_pubkey, &mut state.sql_db.pool().into())
            .await
            .map_err(|e| match e {
                sqlx::Error::RowNotFound => HttpError::not_found(),
                _ => HttpError::from(e),
            })?;

        // Parse the cursor if provided
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

    // Extract user_ids for filtering
    let user_ids: Vec<i32> = user_cursor_map.keys().copied().collect();

    // Internal batch size for DB queries
    const BATCH_SIZE: u16 = 100;

    // Maximum total events to send (None = infinite)
    let max_events = params.limit;
    let mut total_sent: usize = 0;

    let stream = async_stream::stream! {
        // Subscribe to broadcast channel immediately to prevent race condition
        // Events that occur during Phase 1 will be buffered in the channel
        let mut rx = state.event_tx.subscribe();

        // Phase 1: Historical auto-pagination
        // Fetch all historical events in batches until caught up
        // Use per-user cursors to track position for each user independently
        loop {
            let current_user_cursors: Vec<(i32, Option<EventCursor>)> =
                user_cursor_map.iter().map(|(k, cursor)| (*k, *cursor)).collect();

            let events = match EventRepository::get_by_user_cursors(
                current_user_cursors,
                Some(BATCH_SIZE),
                params.reverse,
                params.filter_dir.as_deref(),
                &mut state.sql_db.pool().into(),
            )
            .await
            {
                Ok(events) => events,
                Err(_) => break, // On error, switch to live mode
            };

            let event_count = events.len();

            // Stream each historical event
            for event in events {
                let event_cursor = crate::persistence::sql::event::EventCursor::new(event.created_at, event.id);

                // Update the cursor for this specific user
                user_cursor_map.insert(event.user_id, Some(event_cursor));

                let response = EventResponse::from_entity(&event);
                yield Ok(Event::default()
                    .event(response.event_type.to_string())
                    .data(response.to_sse_data()));

                total_sent += 1;

                // Check if we've reached the user's limit
                if let Some(max) = max_events {
                    if total_sent >= max as usize {
                        return; // Close connection
                    }
                }
            }

            // If we got zero events, all users are caught up with history
            if event_count == 0 {
                // If user specified a limit, close connection (they got what they wanted)
                if max_events.is_some() {
                    return;
                }
                // If not in live mode, close connection (batch mode)
                if !params.live {
                    return;
                }
                // Otherwise, transition to live mode
                break;
            }

            // If we got a partial batch (< BATCH_SIZE but > 0), continue querying
            // Some users might still have more events even though this batch was partial
        }

        // Phase 2: Live mode - stream new events from broadcast channel
        // Only enter if live mode is enabled and no limit was specified
        if params.live && max_events.is_none() {
            while let Ok(event) = rx.recv().await {
                // Filter by user_ids
                if !user_ids.contains(&event.user_id) {
                    continue;
                }

                // Filter by path prefix if specified
                if let Some(ref filter_dir) = params.filter_dir {
                    let path_suffix = event.path.path().as_str();
                    if !path_suffix.starts_with(filter_dir) {
                        continue;
                    }
                }

                // Filter out events we already sent in Phase 1
                if let Some(Some(cursor)) = user_cursor_map.get(&event.user_id) {
                    let is_after_cursor = event.created_at > cursor.timestamp
                        || (event.created_at == cursor.timestamp && event.id > cursor.id);

                    if !is_after_cursor {
                        continue; // Already sent this event
                    }
                }

                // Update this user's cursor
                let event_cursor = crate::persistence::sql::event::EventCursor::new(event.created_at, event.id);
                user_cursor_map.insert(event.user_id, Some(event_cursor));

                let response = EventResponse::from_entity(&event);
                yield Ok(Event::default()
                    .event(response.event_type.to_string())
                    .data(response.to_sse_data()));
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
