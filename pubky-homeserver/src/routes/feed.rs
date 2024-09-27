use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use crate::{error::Result, server::AppState};

pub async fn feed(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse> {
    let limit = params.get("limit").and_then(|l| l.parse::<u16>().ok());
    let cursor = params.get("cursor").map(|c| c.as_str());

    let result = state.db.list_events(limit, cursor)?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
