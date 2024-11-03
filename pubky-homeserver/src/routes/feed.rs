use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Response, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
};
use futures::stream;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use pubky_common::timestamp::Timestamp;

use crate::{
    error::{Error, Result},
    extractors::ListQueryParams,
    server::AppState,
};

pub async fn feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    if let Some(ref cursor) = params.cursor {
        if Timestamp::try_from(cursor.to_string()).is_err() {
            Err(Error::new(
                StatusCode::BAD_REQUEST,
                "Cursor should be valid base32 Crockford encoding of a timestamp".into(),
            ))?
        }
    }

    if params.subscribe {
        // Get the cursor from the last-event-id header or cursor param.
        let cursor = headers
            .get("last-event-id")
            .and_then(|h| h.to_str().ok().map(|s| s.to_string()))
            .or(params.cursor);

        let initial_events = stream::iter(if let Some(cursor) = cursor {
            state
                .db
                .list_events(params.limit, Some(cursor))?
                .iter()
                .map(|(timestamp, event)| {
                    Ok(Event::default()
                        .id(timestamp)
                        .event(event.operation())
                        .data(event.url()))
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        });

        let live_events =
            BroadcastStream::new(state.events.subscribe()).filter_map(|result| match result {
                Ok((timestamp, event)) => Some(Ok::<Event, String>(
                    Event::default()
                        .id(timestamp.to_string())
                        .event(event.operation())
                        .data(event.url()),
                )),
                Err(_) => None,
            });

        let combined_stream = initial_events.chain(live_events);

        return Ok(Sse::new(combined_stream)
            .keep_alive(Default::default())
            .into_response());
    }

    let events = state.db.list_events(params.limit, params.cursor)?;

    let mut result = events
        .iter()
        .map(|(_, e)| e.to_event_line())
        .collect::<Vec<_>>();

    if let Some(last) = events.last() {
        result.push(format!("cursor: {}", last.0))
    };

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
