use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use pubky_common::timestamp::{Timestamp, TimestampError};

use crate::{
    error::{Error, Result},
    extractors::ListQueryParams,
    server::AppState,
};

pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    if let Some(ref cursor) = params.cursor {
        if let Err(timestmap_error) = Timestamp::try_from(cursor.to_string()) {
            let cause = match timestmap_error {
                TimestampError::InvalidEncoding => {
                    "Cursor should be valid base32 Crockford encoding of a timestamp"
                }
                TimestampError::InvalidBytesLength(size) => {
                    &format!("Cursor should be 13 characters long, got: {size}")
                }
            };

            Err(Error::new(StatusCode::BAD_REQUEST, cause.into()))?
        }
    }

    let result = state.db.list_events(params.limit, params.cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
