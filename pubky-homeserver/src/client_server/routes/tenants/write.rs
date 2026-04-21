use axum::{
    body::{Body, HttpBody},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{
    client_server::{extractors::PubkyHost, AppState},
    persistence::{
        files::WriteStreamError,
        sql::{entry::EntryRepository, user::UserEntity, UnifiedExecutor},
    },
    services::user_service::{UserService, FILE_METADATA_SIZE},
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
    state
        .user_service
        .get_or_http_error(public_key, false)
        .await?;
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
    let user = state
        .user_service
        .get_or_http_error(public_key, true)
        .await?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    // Early fail: check Content-Length against the user's storage quota so we
    // can reject before streaming the entire body.
    fail_if_size_hint_exceeds_quota(
        body.size_hint().exact(),
        &user,
        &state.user_service,
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

/// Check whether the Content-Length size hint would exceed the user's storage quota.
/// Returns Ok if there is no size hint, no quota, or the hint fits within the quota.
async fn fail_if_size_hint_exceeds_quota<'a>(
    content_size_hint: Option<u64>,
    user: &UserEntity,
    user_service: &UserService,
    entry_path: &EntryPath,
    executor: &mut UnifiedExecutor<'a>,
) -> HttpResult<()> {
    let content_size_hint = match content_size_hint {
        Some(size) => size,
        None => return Ok(()),
    };

    let existing_entry = EntryRepository::get_by_path(entry_path, executor)
        .await
        .ok();
    let existing_entry_bytes = existing_entry.as_ref().map_or(0, |e| e.content_length);
    let is_new_file = existing_entry.is_none();

    let mut bytes_delta = content_size_hint as i64 - existing_entry_bytes as i64;
    if is_new_file {
        bytes_delta += FILE_METADATA_SIZE as i64;
    }

    if user_service.would_exceed_storage_quota(user, bytes_delta) {
        return Err(HttpError::new_with_message(
            StatusCode::INSUFFICIENT_STORAGE,
            "Disk space quota exceeded. Write operation failed.",
        ));
    }

    Ok(())
}
