use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use pubky_common::timestamp::{Timestamp, TimestampError};

use crate::{
    error::{Error, Result},
    server::AppState,
};

pub async fn feed(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse> {
    let limit = params.get("limit").and_then(|l| l.parse::<u16>().ok());
    let cursor = params.get("cursor").map(|c| c.as_str());

    if let Some(cursor) = cursor {
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

    let result = state.db.list_events(limit, cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
