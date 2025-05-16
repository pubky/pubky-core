use axum::{
    body::{Body, HttpBody},
    extract::{ Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{core::{
    err_if_user_is_invalid::err_if_user_is_invalid, error::{Error, Result}, extractors::PubkyHost, AppState
}, persistence::lmdb::tables::entries::{EntryPath, AsyncInDbTempFileWriter}, shared::WebDavPathAxum};

/// Fail with 507 if `(current + incoming − existing) > quota`.
fn enforce_user_disk_quota(
    existing_bytes: u64,
    incoming_bytes: u64,
    used_bytes: u64,
    quota_bytes: Option<u64>,
) -> Result<()> {
    if let Some(max) = quota_bytes {
        if used_bytes + incoming_bytes.saturating_sub(existing_bytes) > max {
            let bytes_in_mb = 1024.0 * 1024.0;
            let current_mb = used_bytes as f64 / bytes_in_mb;
            let adding_mb = (incoming_bytes - existing_bytes) as f64 / bytes_in_mb;
            let max_mb = max as f64 / bytes_in_mb;
            return Err(Error::new(
                StatusCode::INSUFFICIENT_STORAGE,
                Some(format!(
                    "Quota of {:.1} MB exceeded: you've used {:.1} MB, trying to add {:.1} MB",
                    max_mb, current_mb, adding_mb
                )),
            ));
        }
    }
    Ok(())
}

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathAxum>,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(public_key, &state.db)?;
    let entry_path = EntryPath::new(public_key.clone(), path.0);
    let existing_bytes = state.db.get_entry_content_length(&entry_path)?;

    // Remove entry
    if !state.db.delete_entry2(&entry_path).await? {
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    }

    // Update usage counter
    state
        .db
        .update_data_usage(public_key, -(existing_bytes as i64))?;

    Ok((StatusCode::NO_CONTENT, ()))
}

pub async fn put(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathAxum>,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(public_key, &state.db)?;
    let entry_path = EntryPath::new(public_key.clone(), path.0);

    let existing_entry_bytes = state.db.get_entry_content_length(&entry_path)?;
    let quota_bytes = state.user_quota_bytes;
    let used_bytes = state.db.get_user_data_usage(public_key)?;

    // Upfront check when we have an exact Content-Length
    let hint = body.size_hint().exact();
    if let Some(exact_bytes) = hint {
        enforce_user_disk_quota(existing_entry_bytes, exact_bytes, used_bytes, quota_bytes)?;
    }

    // Stream body to disk first.
    let mut seen_bytes: u64 = 0;
    let mut stream = body.into_data_stream();
    let mut buffer_file_writer = AsyncInDbTempFileWriter::new().await?;
    

    while let Some(chunk) = stream.next().await.transpose()? {
        seen_bytes += chunk.len() as u64;
        enforce_user_disk_quota(existing_entry_bytes, seen_bytes, used_bytes, quota_bytes)?;
        buffer_file_writer.write_chunk(&chunk).await?;
    }
    let buffer_file = buffer_file_writer.complete().await?;

    // Write file on disk to db
    state.db.write_entry2(&entry_path, &buffer_file).await?;
    let delta = buffer_file.len() as i64 - existing_entry_bytes as i64;
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
