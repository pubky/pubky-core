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
    persistence::lmdb::tables::files::FileLocation,
    shared::webdav::{EntryPath, WebDavPathPubAxum},
};
use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

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

/// A stream wrapper that enforces quota checking for each chunk
struct QuotaEnforcingStream<S> {
    inner: S,
    existing_bytes: u64,
    seen_bytes: u64,
    used_bytes: u64,
    quota_bytes: Option<u64>,
}

impl<S> QuotaEnforcingStream<S> {
    fn new(
        inner: S,
        existing_bytes: u64,
        used_bytes: u64,
        quota_bytes: Option<u64>,
    ) -> Self {
        Self {
            inner,
            existing_bytes,
            seen_bytes: 0,
            used_bytes,
            quota_bytes,
        }
    }
}

impl<S> Stream for QuotaEnforcingStream<S>
where
    S: Stream<Item = Result<Bytes, anyhow::Error>> + Unpin,
{
    type Item = Result<Bytes, anyhow::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.seen_bytes += chunk.len() as u64;
                if let Err(e) = enforce_user_disk_quota(
                    self.existing_bytes,
                    self.seen_bytes,
                    self.used_bytes,
                    self.quota_bytes,
                ) {
                    return Poll::Ready(Some(Err(anyhow::anyhow!("Quota exceeded: {:?}", e))));
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key();
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    let entry = state.file_service.get_info(&entry_path).await?
        .ok_or_else(|| Error::with_status(StatusCode::NOT_FOUND))?;
    state.file_service.delete(&entry_path).await?;

    // Update usage counter
    state
        .db
        .update_data_usage(public_key, -(entry.content_length() as i64))?;

    Ok((StatusCode::NO_CONTENT, ()))
}

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
    let quota_bytes = state.user_quota_bytes;
    let used_bytes = state.db.get_user_data_usage(public_key)?;

    // Upfront check when we have an exact Content-Length
    let hint = body.size_hint().exact();
    if let Some(exact_bytes) = hint {
        enforce_user_disk_quota(existing_entry_bytes, exact_bytes, used_bytes, quota_bytes)?;
    }

    // Convert body stream to the format expected by file_service
    let body_stream = body.into_data_stream();
    let converted_stream = body_stream.map(|chunk_result| {
        chunk_result.map_err(|e| anyhow::anyhow!("Body stream error: {}", e))
    });

    // Wrap with quota enforcement
    let quota_stream = QuotaEnforcingStream::new(
        converted_stream,
        existing_entry_bytes,
        used_bytes,
        quota_bytes,
    );

    // Write using file_service (defaulting to LMDB for backward compatibility)
    let entry = state
        .file_service
        .write_stream(&entry_path, FileLocation::LMDB, quota_stream)
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
