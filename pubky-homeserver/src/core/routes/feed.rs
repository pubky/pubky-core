use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use crate::{core::{extractors::ListQueryParams, AppState}, persistence::sql::event::EventRepository, shared::{HttpError, HttpResult}
};

pub async fn feed(
    State(state): State<AppState>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {

    let cursor = match params.cursor {
        Some(cursor) => cursor,
        None => "0".to_string(),
    };

    let cursor = match EventRepository::parse_cursor(cursor.as_str(), &mut state.sql_db.pool().into()).await {
        Ok(cursor) => cursor,
        Err(e) => return Err(HttpError::bad_request("Invalid cursor")),
    };

    let events = EventRepository::get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into()).await?;
    let result = events.iter().map(|event| format!("{} pubky://{}", event.event_type, event.path.as_str())).collect::<Vec<String>>();

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap())
}
