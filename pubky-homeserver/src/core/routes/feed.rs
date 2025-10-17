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
use std::convert::Infallible;

use crate::{
    core::{extractors::ListQueryParams, AppState},
    persistence::sql::event::EventRepository,
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

    let events =
        EventRepository::get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
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
/// - **Note:** Only entered if `limit` parameter is omitted (infinite stream)
///
/// ## Query Parameters
/// - `cursor` (optional): Starting point for event stream. Format: "timestamp:id" or legacy timestamp.
///   Default: "0" (start from beginning)
/// - `limit` (optional): Maximum total events to send before closing connection.
///   - If **omitted**: Stream all historical events + enter live mode (infinite stream)
///   - If **specified**: Send up to N events total, then close connection
/// - `user` (optional): Reserved for future user-specific filtering (currently unused)
///
/// ## SSE Response Format
/// Each event is sent as:
/// ```text
/// event: PUT
/// data: pubky://user_pubkey/pub/example.txt
/// data: cursor: 00331BD814YCT:42
/// ```
pub async fn feed_stream(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let cursor = match params.cursor {
        Some(cursor) => cursor,
        None => "0".to_string(),
    };

    let mut cursor =
        match EventRepository::parse_cursor(cursor.as_str(), &mut state.sql_db.pool().into()).await
        {
            Ok(cursor) => cursor,
            Err(_e) => return Err(HttpError::bad_request("Invalid cursor")),
        };

    // Internal batch size for DB queries. We may want to allow caller to specify this.
    const BATCH_SIZE: u16 = 100;

    // Maximum total events to send (None = infinite)
    let max_events = params.limit;
    let mut total_sent: usize = 0;

    let stream = async_stream::stream! {
        // Phase 1: Historical auto-pagination
        // Fetch all historical events in batches until caught up
        loop {
            let events = match EventRepository::get_by_cursor(
                Some(cursor),
                Some(BATCH_SIZE),
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
                cursor = crate::persistence::sql::event::EventCursor::new(event.created_at, event.id);
                let event_data = format!(
                    "pubky://{}\ncursor: {}",
                    event.path.as_str(),
                    cursor
                );
                yield Ok(Event::default()
                    .event(event.event_type.to_string())
                    .data(event_data));

                total_sent += 1;

                // Check if we've reached the user's limit
                if let Some(max) = max_events {
                    if total_sent >= max as usize {
                        return; // Close connection
                    }
                }
            }

            // If we got a partial batch or empty result, we're caught up with history
            if event_count < BATCH_SIZE as usize {
                // If user specified a limit, close connection (they got what they wanted)
                if max_events.is_some() {
                    return;
                }
                // Otherwise, transition to live mode
                break;
            }
        }

        // Phase 2: Live mode - stream new events from broadcast channel
        // Only enter if no limit was specified (infinite stream)
        if max_events.is_none() {
            let mut rx = state.event_tx.subscribe();
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Only send events after our cursor
                        let is_after_cursor = event.created_at > cursor.timestamp
                            || (event.created_at == cursor.timestamp && event.id > cursor.id);

                        if !is_after_cursor {
                            continue;
                        }

                        cursor = crate::persistence::sql::event::EventCursor::new(event.created_at, event.id);
                        let event_data = format!(
                            "pubky://{}\ncursor: {}",
                            event.path.as_str(),
                            cursor
                        );
                        yield Ok(Event::default()
                            .event(event.event_type.to_string())
                            .data(event_data));
                    }
                    Err(_) => {
                        // Channel closed or lagged - connection will close
                        break;
                    }
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
