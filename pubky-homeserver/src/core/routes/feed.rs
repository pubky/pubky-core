use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use pubky_common::timestamp::Timestamp;

use crate::core::{
    error::{Error, Result},
    extractors::ListQueryParams,
    AppState,
};

pub async fn feed(
    State(state): State<AppState>,
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

    let result = state.db.list_events(params.limit, params.cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
