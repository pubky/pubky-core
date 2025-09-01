use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use pubky_common::timestamp::Timestamp;

use crate::{
    client_server::{extractors::ListQueryParams, AppState},
    shared::{HttpError, HttpResult},
};

pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {
    if let Some(ref cursor) = params.cursor {
        if Timestamp::try_from(cursor.to_string()).is_err() {
            return Err(HttpError::bad_request(
                "Cursor should be valid base32 Crockford encoding of a timestamp",
            ));
        }
    }

    let result = state.db.list_events(params.limit, params.cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
