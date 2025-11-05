use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};
use pkarr::PublicKey;
use std::str::FromStr;
use std::time::Duration;

use crate::{
    core::{extractors::ListQueryParams, AppState},
    persistence::sql::{event::EventRepository, user::UserRepository},
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

    // Parse optional user filter
    let user_id_filter: Option<i32> = if let Some(user_pubkey_str) = params.user {
        let user_pubkey = PublicKey::from_str(&user_pubkey_str)
            .map_err(|_| HttpError::bad_request("Invalid user public key"))?;
        Some(UserRepository::get_id(&user_pubkey, &mut state.sql_db.pool().into()).await?)
    } else {
        None
    };

    // Parse timeout (default 0 = no wait, max 60 seconds)
    let timeout_secs = params.timeout.unwrap_or(0).min(60);

    // Fetch existing events
    let events = if let Some(user_id) = user_id_filter {
        EventRepository::get_by_user_and_cursor(
            user_id,
            Some(cursor),
            params.limit,
            &mut state.sql_db.pool().into(),
        )
        .await?
    } else {
        EventRepository::get_by_cursor(Some(cursor), params.limit, &mut state.sql_db.pool().into())
            .await?
    };

    // If we have events, return immediately
    if !events.is_empty() {
        return Ok(format_response(&events));
    }

    // If no timeout requested, return empty response
    if timeout_secs == 0 {
        return Ok(empty_response(cursor));
    }

    // Long-poll: wait for new events
    let mut rx = state.event_tx.subscribe();
    let timeout_duration = Duration::from_secs(timeout_secs);
    let deadline = tokio::time::sleep(timeout_duration);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            event_result = rx.recv() => {
                match event_result {
                    Ok(event) => {
                        if event.id <= cursor {
                            continue; // Too old
                        }
                        if let Some(user_id) = user_id_filter {
                            if event.user_id != user_id {
                                continue; // Wrong user
                            }
                        }
                        return Ok(format_response(&vec![event]));
                    }
                    Err(_) => {
                        return Ok(empty_response(cursor));
                    }
                }
            }
            _ = &mut deadline => {
                return Ok(empty_response(cursor));
            }
        }
    }
}

fn format_response(events: &[crate::persistence::sql::event::EventEntity]) -> Response<Body> {
    // events must be non-empty when calling this function
    debug_assert!(
        !events.is_empty(),
        "format_response called with empty events"
    );

    let mut result = events
        .iter()
        .map(|event| format!("{} pubky://{}", event.event_type, event.path.as_str()))
        .collect::<Vec<String>>();

    // Get cursor from last event (guaranteed to exist due to assertion)
    let next_cursor = events.last().unwrap().id.to_string();
    result.push(format!("cursor: {}", next_cursor));

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(result.join("\n")))
        .unwrap()
}

fn empty_response(cursor: i64) -> Response<Body> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(format!("cursor: {}", cursor)))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::event::{EventEntity, EventType};
    use crate::shared::webdav::WebDavPath;
    use axum::body::to_bytes;
    use pkarr::Keypair;
    use sqlx::types::chrono::Utc;

    #[tokio::test]
    async fn test_format_response() {
        let user_pubkey = Keypair::random().public_key();
        let path = crate::shared::webdav::EntryPath::new(
            user_pubkey.clone(),
            WebDavPath::new("/pub/test.txt").unwrap(),
        );

        let events = vec![
            EventEntity {
                id: 1,
                user_id: 1,
                user_pubkey: user_pubkey.clone(),
                event_type: EventType::Put,
                path: path.clone(),
                created_at: Utc::now().naive_utc(),
            },
            EventEntity {
                id: 2,
                user_id: 1,
                user_pubkey,
                event_type: EventType::Delete,
                path: path.clone(),
                created_at: Utc::now().naive_utc(),
            },
        ];

        let response = format_response(&events);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );

        let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();
        let lines: Vec<&str> = body_str.split('\n').collect();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("PUT pubky://"));
        assert!(lines[0].contains("/pub/test.txt"));
        assert!(lines[1].starts_with("DEL pubky://"));
        assert_eq!(lines[2], "cursor: 2");
    }

    #[tokio::test]
    async fn test_empty_response() {
        let cursor = 42;
        let response = empty_response(cursor);

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );

        let body_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body_bytes.to_vec()).unwrap();

        assert_eq!(body_str, "cursor: 42");
    }

    #[test]
    fn test_empty_response_different_cursors() {
        for cursor in [0, 1, 100, 999999] {
            let response = empty_response(cursor);
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
}
