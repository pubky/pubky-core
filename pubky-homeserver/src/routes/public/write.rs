use std::io::Write;

use futures_util::stream::StreamExt;

use axum::{body::Body, extract::State, http::StatusCode, response::IntoResponse};
use tower_cookies::Cookies;

use crate::{
    core::AppState,
    error::{Error, Result},
    extractors::{EntryPath, Pubky},
};

use super::{authorize, verify};

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();

    verify(&path)?;
    authorize(&mut state, cookies, &public_key, &path)?;

    // TODO: should we wrap this with `tokio::task::spawn_blocking` in case it takes too long?
    let deleted = state.db.delete_entry(&public_key, &path)?;

    if !deleted {
        // TODO: if the path ends with `/` return a `CONFLICT` error?
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    };

    Ok(())
}

pub async fn put(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();

    verify(&path)?;
    authorize(&mut state, cookies, &public_key, &path)?;

    let mut entry_writer = state.db.write_entry(&public_key, &path)?;

    let mut stream = body.into_data_stream();
    while let Some(next) = stream.next().await {
        let chunk = next?;
        entry_writer.write_all(&chunk)?;
    }

    let _entry = entry_writer.commit()?;

    // TODO: return relevant headers, like Etag?

    Ok(())
}
