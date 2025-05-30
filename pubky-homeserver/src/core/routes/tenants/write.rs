use axum::{
    body::{Body, HttpBody},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{
    core::{
        err_if_user_is_invalid::err_if_user_is_invalid,
        error::{Error, Result},
        extractors::PubkyHost,
        AppState,
    },
    persistence::{files::{is_size_hint_exceeding_quota}, lmdb::tables::files::FileLocation},
    shared::webdav::{EntryPath, WebDavPathPubAxum},
};

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    let entry = state
        .file_service
        .get_info(&entry_path)
        .await?
        .ok_or_else(|| Error::with_status(StatusCode::NOT_FOUND))?;
    state.file_service.delete(&entry_path).await?;

    // Update usage counter
    state
        .db
        .update_data_usage(public_key, -(entry.content_length() as i64))?;

    Ok((StatusCode::NO_CONTENT, ()))
}

#[axum::debug_handler]
pub async fn put(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(public_key, &state.db, true)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());
    let existing_entry_bytes = state.db.get_entry_content_length(&entry_path)?;

    // Check if the size hint exceeds the quota so we can fail early
    if let Some(size_hint) = body.size_hint().exact() {
        if let Some(user_quota_bytes) = state.user_quota_bytes {
            if is_size_hint_exceeding_quota(size_hint, &state.db, &entry_path, user_quota_bytes)? {
                let max_allowed_mb = user_quota_bytes as f64 / 1024.0 / 1024.0;
                return Err(Error::new(StatusCode::INSUFFICIENT_STORAGE, 
                    Some(format!("Disk space quota of {max_allowed_mb:.1} MB exceeded. Write operation failed."))));
            }
        }
    }


    // Convert body stream to the format expected by file_service
    let body_stream = body.into_data_stream();
    let converted_stream = body_stream
        .map(|chunk_result| chunk_result.map_err(|e| anyhow::anyhow!("Body stream error: {}", e)));


    // Write using file_service (defaulting to LMDB for backward compatibility)
    let entry = state
        .file_service
        .write_stream(&entry_path, FileLocation::LMDB, converted_stream)
        .await
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, Some(e.to_string())))?;

    // Update usage counter based on the actual file size
    let delta = entry.content_length() as i64 - existing_entry_bytes as i64;
    state.db.update_data_usage(public_key, delta)?;

    Ok((StatusCode::CREATED, ()))
}

#[cfg(test)]
mod quota_unit_tests {
    use super::enforce_user_disk_quota;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn allows_exact_quota() {
        // existing=0, incoming=1024, used=0, quota=Some(1024)
        assert!(enforce_user_disk_quota(0, 1024, 0, Some(1024)).is_ok());
    }

    #[tokio::test]
    async fn blocks_when_over_quota() {
        let err = enforce_user_disk_quota(0, 1025, 0, Some(1024)).unwrap_err();
        // convert to a real HTTP Response
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);
    }

    #[tokio::test]
    async fn considers_existing_size() {
        // existing=800, incoming=600, used=0, quota=1000 => delta = max(0,600-800)=0
        assert!(enforce_user_disk_quota(800, 600, 0, Some(1000)).is_ok());
    }

    #[tokio::test]
    async fn unlimited_when_quota_none() {
        assert!(enforce_user_disk_quota(0, 10_000_000, 0, None).is_ok());
    }
}
