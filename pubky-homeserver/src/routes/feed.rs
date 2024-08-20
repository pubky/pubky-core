use std::collections::HashMap;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use crate::{
    database::{tables::events::Event, MAX_LIST_LIMIT},
    error::Result,
    server::AppState,
};

pub async fn feed(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse> {
    let txn = state.db.env.read_txn()?;

    let limit = params
        .get("limit")
        .and_then(|l| l.parse::<u16>().ok())
        .unwrap_or(MAX_LIST_LIMIT)
        .min(MAX_LIST_LIMIT);

    let mut cursor = params
        .get("cursor")
        .map(|c| c.as_str())
        .unwrap_or("0000000000000");

    if cursor.len() < 13 {
        cursor = "0000000000000"
    }

    let mut result: Vec<String> = vec![];
    let mut next_cursor = "".to_string();

    for _ in 0..limit {
        match state.db.tables.events.get_greater_than(&txn, cursor)? {
            Some((timestamp, event_bytes)) => {
                let event = Event::deserialize(event_bytes)?;

                let line = format!("{} {}", event.operation(), event.url());
                next_cursor = timestamp.to_string();

                result.push(line);
            }
            None => break,
        };
    }

    if !result.is_empty() {
        result.push(format!("cursor: {next_cursor}"))
    }

    txn.commit()?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
