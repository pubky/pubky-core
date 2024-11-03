use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::{
        sse::{Event, Sse},
        IntoResponse,
    },
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use pubky_common::timestamp::Timestamp;

use crate::{
    error::{Error, Result},
    extractors::ListQueryParams,
    server::AppState,
};

pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    if params.subscribe {
        let rx = state.events.subscribe();

        let stream = BroadcastStream::new(rx).filter_map(|result| match result {
            Ok((timestamp, event)) => Some(Ok::<Event, String>(
                Event::default()
                    .id(timestamp.to_string())
                    .event(event.operation())
                    .data(event.url()),
            )),
            Err(_) => None,
        });

        return Ok(Sse::new(stream)
            .keep_alive(Default::default())
            .into_response());
    }

    if let Some(ref cursor) = params.cursor {
        if Timestamp::try_from(cursor.to_string()).is_err() {
            Err(Error::new(
                StatusCode::BAD_REQUEST,
                "Cursor should be valid base32 Crockford encoding of a timestamp".into(),
            ))?
        }
    }

    let result = state.db.list_events(params.limit, params.cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
