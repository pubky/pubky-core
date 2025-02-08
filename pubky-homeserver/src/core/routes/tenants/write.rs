use std::io::Write;

use futures_util::stream::StreamExt;

use axum::{
    body::Body,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::core::{
    error::{Error, Result},
    extractors::PubkyHost,
    AppState,
};

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();

    // TODO: should we wrap this with `tokio::task::spawn_blocking` in case it takes too long?
    let deleted = state.db.delete_entry(&public_key, path.0.path())?;

    if !deleted {
        // TODO: if the path ends with `/` return a `CONFLICT` error?
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    };

    Ok(())
}

pub async fn put(
    State(mut state): State<AppState>,
    pubky: PubkyHost,
    path: OriginalUri,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();

    let mut entry_writer = state.db.write_entry(&public_key, path.0.path())?;

    let mut stream = body.into_data_stream();
    while let Some(next) = stream.next().await {
        let chunk = next?;
        entry_writer.write_all(&chunk)?;
    }

    let _entry = entry_writer.commit()?;

    // TODO: return relevant headers, like Etag?

    Ok(())
}
