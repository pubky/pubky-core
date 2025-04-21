use std::io::Write;

use axum::{
    body::{Body, HttpBody},
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::core::{
    err_if_user_is_invalid::err_if_user_is_invalid,
    error::{Error, Result},
    extractors::PubkyHost,
    AppState,
};

/// Bytes already stored at `path` for this user (0 if none).
fn existing_len(state: &AppState, pk: &pubky_common::crypto::PublicKey, path: &str) -> Result<u64> {
    let rtxn = state.db.env.read_txn()?;
    Ok(state
        .db
        .get_entry(&rtxn, pk, path)?
        .map(|e| e.content_length() as u64)
        .unwrap_or(0))
    // read‑only txns auto‑abort on drop, so no explicit commit needed
}

/// Fail with 507 if `(current + incoming − existing) > quota`.
fn enforce_quota(existing: u64, incoming: u64, current: u64, quota: Option<u64>) -> Result<()> {
    if let Some(max) = quota {
        if current + incoming.saturating_sub(existing) > max {
            return Err(Error::new(
                StatusCode::INSUFFICIENT_STORAGE,
                Some(format!(
                    "Storage quota exceeded ({:.1} MB)",
                    max as f64 / (1024.0 * 1024.0)
                )),
            ));
        }
    }
    Ok(())
}

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
) -> Result<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db)?;
    let pk = pubky.public_key();
    let full_path = path.0.path();
    let existing = existing_len(&state, pk, full_path)?;

    // Remove entry
    if !state.db.delete_entry(pk, full_path)? {
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    }

    // Update usage counter
    state.db.update_data_usage(pk, -(existing as i64))?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn put(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
    body: Body,
) -> Result<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db)?;
    let pk = pubky.public_key().clone();
    let full_path = path.0.path();
    let existing = existing_len(&state, &pk, full_path)?;
    let quota = state.user_quota_bytes;
    let used = state.db.get_user_data_usage(&pk)?;

    // Upfront check when we have an exact Content‑Length
    let hint = body.size_hint().exact();
    if let Some(exact) = hint {
        enforce_quota(existing, exact, used, quota)?;
    }

    // Stream body, counting only when we didn’t have a hint
    let mut writer = state.db.write_entry(&pk, full_path)?;
    let mut seen: u64 = 0;
    let mut stream = body.into_data_stream();

    while let Some(chunk) = stream.next().await.transpose()? {
        if hint.is_none() {
            seen += chunk.len() as u64;
            enforce_quota(existing, seen, used, quota)?;
        }
        writer.write_all(&chunk)?;
    }

    // Commit & bump usage
    let entry = writer.commit()?;
    let delta = entry.content_length() as i64 - existing as i64;
    state.db.update_data_usage(&pk, delta)?;

    Ok(StatusCode::CREATED)
}

#[cfg(test)]
mod quota_unit_tests {
    use super::enforce_quota;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn allows_exact_quota() {
        // existing=0, incoming=1024, used=0, quota=Some(1024)
        assert!(enforce_quota(0, 1024, 0, Some(1024)).is_ok());
    }

    #[tokio::test]
    async fn blocks_when_over_quota() {
        let err = enforce_quota(0, 1025, 0, Some(1024)).unwrap_err();
        // convert to a real HTTP Response
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);
    }

    #[tokio::test]
    async fn considers_existing_size() {
        // existing=800, incoming=600, used=0, quota=1000 => delta = max(0,600-800)=0
        assert!(enforce_quota(800, 600, 0, Some(1000)).is_ok());
    }

    #[tokio::test]
    async fn unlimited_when_quota_none() {
        assert!(enforce_quota(0, 10_000_000, 0, None).is_ok());
    }
}
