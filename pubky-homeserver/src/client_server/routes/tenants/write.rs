use axum::{
    body::{Body, HttpBody},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{
    client_server::{
        err_if_user_is_invalid::get_user_or_http_error, extractors::PubkyHost, AppState,
    },
    persistence::{
        files::WriteStreamError,
        sql::{entry::EntryRepository, user::UserRepository, UnifiedExecutor},
    },
    shared::{
        webdav::{EntryPath, WebDavPathPubAxum},
        HttpError, HttpResult,
    },
};

pub async fn delete(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key();
    get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    state.file_service.delete(&entry_path).await?;
    Ok((StatusCode::NO_CONTENT, ()))
}

pub async fn put(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
    body: Body,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key();
    get_user_or_http_error(public_key, &mut state.sql_db.pool().into(), true).await?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    // Check if the size hint exceeds the quota so we can fail early
    let content_size_hint = body.size_hint().exact();
    fail_if_size_hint_bigger_than_user_quota(
        content_size_hint,
        state.user_quota_bytes,
        &entry_path,
        &mut state.sql_db.pool().into(),
    )
    .await?;

    // Convert body stream to the format expected by file_service
    let body_stream = body.into_data_stream();
    let converted_stream =
        body_stream.map(|chunk_result| chunk_result.map_err(WriteStreamError::Axum));

    state
        .file_service
        .write_stream(&entry_path, converted_stream)
        .await?;
    Ok((StatusCode::CREATED, ()))
}

/// Checks if the size hint exceeds the quota so we can fail early.
/// Will return an error if the size hint exceeds the quota.
/// Will return Ok if the size hint is smaller than the quota.
/// Will return Ok if there is no quota.
/// Will return Ok if there is no size hint.
pub async fn fail_if_size_hint_bigger_than_user_quota<'a>(
    content_size_hint: Option<u64>,
    user_quota_bytes: Option<u64>,
    entry_path: &EntryPath,
    executor: &mut UnifiedExecutor<'a>,
) -> HttpResult<()> {
    let content_size_hint = match content_size_hint {
        Some(size_hint) => size_hint,
        None => return Ok(()), // No size hint, so we can't check
    };
    let max_allowed_bytes = match user_quota_bytes {
        Some(user_quota_bytes) => user_quota_bytes,
        None => return Ok(()), // No quota, so all good
    };

    let existing_entry_bytes = EntryRepository::get_by_path(entry_path, executor)
        .await
        .map(|entry| entry.content_length)
        .unwrap_or(0);
    let user_already_used_bytes = UserRepository::get(entry_path.pubkey(), executor)
        .await
        .map(|user| user.used_bytes)?;

    let is_quota_exceeded = user_already_used_bytes
        + content_size_hint.saturating_sub(existing_entry_bytes)
        > max_allowed_bytes;

    if is_quota_exceeded {
        let max_allowed_mb = max_allowed_bytes as f64 / 1024.0 / 1024.0;
        return Err(HttpError::new_with_message(
            StatusCode::INSUFFICIENT_STORAGE,
            format!("Disk space quota of {max_allowed_mb:.1} MB exceeded. Write operation failed."),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use crate::{persistence::sql::SqlDb, shared::webdav::WebDavPath};

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_if_size_hint_all_good() {
        let db = SqlDb::test().await;
        let pubkey = Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let entry = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());
        let body = Body::from("test");

        fail_if_size_hint_bigger_than_user_quota(
            body.size_hint().exact(),
            Some(1024),
            &entry,
            &mut db.pool().into(),
        )
        .await
        .expect("should not fail");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_if_size_hint_bigger_than_quota() {
        let db = SqlDb::test().await;
        let pubkey = Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let entry = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());
        let body = Body::from("test");

        fail_if_size_hint_bigger_than_user_quota(
            body.size_hint().exact(),
            Some(1),
            &entry,
            &mut db.pool().into(),
        )
        .await
        .expect_err("should fail");
    }
}
