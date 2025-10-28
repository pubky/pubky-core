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
        AppState, extractors::{EventStreamQueryParams, ListQueryParams}
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
/// This endpoint implements a two-phase streaming approach:
///
/// ## Phase 1: Historical Auto-pagination
/// - Fetches all historical events from the cursor onwards
/// - Queries the database in batches of 100 events (internal optimization)
/// - Streams events progressively as they're fetched
/// - Continues until caught up (empty result or partial batch indicates end of history)
///
/// ## Phase 2: Live Mode
/// - Subscribes to the broadcast channel for new events
/// - Streams new events in real-time as they occur
/// - Connection stays open indefinitely
/// - Each event includes updated cursor for client state tracking
/// - **Note:** Only entered if `limit` parameter is omitted (infinite stream) AND `reverse` is false
///
/// ## Query Parameters
/// - `user` (**REQUIRED**): One or more user public keys to filter events for. Can be repeated multiple times.
///   - Format: z-base-32 encoded public key (e.g., "o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo")
///   - Single user: `?user=pubkey1`
///   - Multiple users: `?user=pubkey1&user=pubkey2&user=pubkey3`
///   - Maximum: 50 users per request
///   - The endpoint uses the `(user, created_at, id)` database index for efficient querying
/// - `cursor` (optional): Starting point for event stream. Format: "timestamp:id" or legacy timestamp.
///   Default: "0" (start from beginning)
/// - `limit` (optional): Maximum total events to send before closing connection.
///   - If **omitted**: Stream all historical events + enter live mode (infinite stream, unless `reverse=true`)
///   - If **specified**: Send up to N events total, then close connection
/// - `reverse` (optional): Return events in reverse chronological order (newest first).
///   Default: false (oldest first)
///   - When `true`, only historical events are returned (connection closes after reaching earliest event)
///   - Live mode is not available with `reverse=true` (doesn't make sense to stream backwards in time)
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
                user_cursor_map.iter().map(|(k, v)| (*k, *v)).collect();

            let events = match EventRepository::get_by_user_cursors(
                current_user_cursors,
                Some(BATCH_SIZE),
                params.reverse,
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
                // If reverse mode, we've reached the end of historical data - close connection
                if params.reverse {
                    return;
                }
                // Otherwise, transition to live mode
                break;
            }

            // If we got a partial batch (< BATCH_SIZE but > 0), continue querying
            // Some users might still have more events even though this batch was partial
        }

        // Phase 2: Live mode - stream new events from broadcast channel
        // Only enter if no limit was specified (infinite stream) and not in reverse mode
        if max_events.is_none() && !params.reverse {
            while let Ok(event) = rx.recv().await {
                // Filter by user_ids
                if !user_ids.contains(&event.user_id) {
                    continue;
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
