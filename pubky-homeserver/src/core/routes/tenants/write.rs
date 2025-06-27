use axum::{
    body::{Body, HttpBody},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{
    core::{err_if_user_is_invalid::err_if_user_is_invalid, extractors::PubkyHost, AppState},
    persistence::{files::WriteStreamError},
    shared::{
        webdav::{EntryPath, WebDavPathPubAxum},
        HttpError, HttpResult,
    },
};

use crate::persistence::files::FileIoError;
use crate::persistence::lmdb::LmDB;

pub async fn delete(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
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
    err_if_user_is_invalid(public_key, &state.db, true)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    // Check if the size hint exceeds the quota so we can fail early
    fail_if_size_hint_bigger_than_user_quota(
        &body,
        &state.db,
        state.user_quota_bytes,
        &entry_path,
    )?;

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
pub fn fail_if_size_hint_bigger_than_user_quota(
    body: &Body,
    db: &LmDB,
    user_quota_bytes: Option<u64>,
    entry_path: &EntryPath,
) -> HttpResult<()> {
    let content_size_hint = match body.size_hint().exact() {
        Some(size_hint) => size_hint,
        None => return Ok(()), // No size hint, so we can't check
    };
    let max_allowed_bytes = match user_quota_bytes {
        Some(user_quota_bytes) => user_quota_bytes,
        None => return Ok(()), // No quota, so all good
    };

    let existing_entry_bytes = db.get_entry_content_length_default_zero(entry_path)?;
    let user_already_used_bytes = match db.get_user_data_usage(entry_path.pubkey())? {
        Some(bytes) => bytes,
        None => return Err(FileIoError::NotFound.into()),
    };

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
    use pkarr::Keypair;

    use crate::shared::webdav::WebDavPath;

    use super::*;

    #[test]
    fn test_if_size_hint_all_good() {
        let db = LmDB::test();
        let pubkey = Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();
        let entry = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());
        let body = Body::from("test");

        fail_if_size_hint_bigger_than_user_quota(&body, &db, Some(1024), &entry)
            .expect("should not fail");
    }

    #[test]
    fn test_if_size_hint_bigger_than_quota() {
        let db = LmDB::test();
        let pubkey = Keypair::random().public_key();
        db.create_user(&pubkey).unwrap();
        let entry = EntryPath::new(pubkey, WebDavPath::new("/test.txt").unwrap());
        let body = Body::from("test");

        fail_if_size_hint_bigger_than_user_quota(&body, &db, Some(1), &entry)
            .expect_err("should fail");
    }
}
