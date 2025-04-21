use std::io::Write;

use futures_util::stream::StreamExt;

use axum::{
    body::{Body, HttpBody},
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::core::{
    err_if_user_is_invalid::err_if_user_is_invalid,
    error::{Error, Result},
    extractors::PubkyHost,
    AppState,
};

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
) -> Result<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db)?;
    let public_key = pubky.public_key().clone();
    let full_path = path.0.path();

    // Gather accounting info
    let rtxn = state.db.env.read_txn()?;
    let existing_len = state
        .db
        .get_entry(&rtxn, &public_key, full_path)?
        .map(|e| e.content_length() as u64)
        .unwrap_or(0);
    rtxn.commit()?;

    // TODO: should we wrap this with `tokio::task::spawn_blocking` in case it takes too long?
    let deleted = state.db.delete_entry(&public_key, full_path)?;

    match deleted {
        false => return Err(Error::with_status(StatusCode::NOT_FOUND)),
        true => {
            state
                .db
                .update_data_usage(&public_key, -(existing_len as i64))?;
        }
    }

    Ok(())
}

pub async fn put(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
    body: Body,
) -> Result<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db)?;
    let public_key = pubky.public_key().clone();
    let full_path = path.0.path();

    // Gather accounting info
    let existing_len: u64;
    {
        let rtxn = state.db.env.read_txn()?;
        existing_len = state
            .db
            .get_entry(&rtxn, &public_key, full_path)
            .expect("a")
            .map(|e| e.content_length() as u64)
            .unwrap_or(0);
        rtxn.commit()?;
    }

    let quota_bytes = state.user_quota_bytes;
    let current_usage = state.db.get_user_data_usage(&public_key)?;

    // Compute (or estimate) the size to be written
    let mut incoming_len: u64 = 0;

    // If the client sent Content‑Length we can check immediately.
    if let Some(cl) = body.size_hint().exact() {
        incoming_len = cl;
        if let Some(max) = quota_bytes {
            if current_usage + incoming_len.saturating_sub(existing_len) > max {
                return Err(Error::new(
                    StatusCode::INSUFFICIENT_STORAGE,
                    Some(format!(
                        "Storage quota exceeded ({} MB max)",
                        max / 1_048_576
                    )),
                ));
            }
        }
    }

    // Do the write
    let mut writer = state.db.write_entry(&public_key, full_path)?;
    let mut stream = body.into_data_stream();

    while let Some(next) = stream.next().await {
        let chunk = next?;

        // If we *didn’t* know the length up‑front, count it as we go
        if incoming_len == 0 {
            incoming_len += chunk.len() as u64;
            if let Some(max) = quota_bytes {
                if current_usage + incoming_len.saturating_sub(existing_len) > max {
                    return Err(Error::new(
                        StatusCode::INSUFFICIENT_STORAGE,
                        "Storage quota exceeded".into(),
                    ));
                }
            }
        }

        writer.write_all(&chunk)?;
    }

    // Final length when Content‑Length was unknown.
    let entry = writer.commit()?;

    // Update disk usage counter
    let new_len = entry.content_length() as u64;
    let delta = (new_len as i64) - (existing_len as i64);
    state.db.update_data_usage(&public_key, delta)?;

    Ok(())
}
