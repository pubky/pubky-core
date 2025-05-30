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
    persistence::{files::{is_size_hint_exceeding_quota, WriteFileFromStreamError, WriteStreamError}, lmdb::tables::files::FileLocation},
    shared::webdav::{EntryPath, WebDavPathPubAxum},
};

pub async fn delete(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    if let None = state
        .file_service
        .get_info(&entry_path)
        .await?{
            return Err(Error::with_status(StatusCode::NOT_FOUND));
        };
    state.file_service.delete(&entry_path).await?;
    Ok((StatusCode::NO_CONTENT, ()))
}

pub async fn put(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(public_key, &state.db, true)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

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
        .map(|chunk_result| chunk_result.map_err(|e| WriteStreamError::Axum(e)));


    // Write using file_service (defaulting to LMDB for backward compatibility)
    if let Err(e) = state.file_service
        .write_stream(&entry_path, FileLocation::LMDB, converted_stream)
        .await {
            match e {
                WriteFileFromStreamError::StreamBroken(WriteStreamError::DiskSpaceQuotaExceeded) => {
                    return Err(Error::new(StatusCode::INSUFFICIENT_STORAGE, Some("Disk space quota exceeded. Write operation failed.".to_string())));
                }
                WriteFileFromStreamError::StreamBroken(_) => {
                    return Err(Error::new(StatusCode::BAD_REQUEST, Some("Stream broken. Write operation failed.".to_string())));
                }
                _ => {
                    tracing::error!("Write operation failed: {:?}", e);
                    return Err(Error::new(StatusCode::INTERNAL_SERVER_ERROR, Some("Internal server error.".to_string())));
                }
            }
        }

    Ok((StatusCode::CREATED, ()))
}
