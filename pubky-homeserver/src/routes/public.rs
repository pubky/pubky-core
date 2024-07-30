use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    RequestExt, Router,
};
use axum_extra::body::AsyncReadBody;
use futures_util::stream::StreamExt;
use tower_cookies::Cookies;

use tracing::debug;

use pubky_common::crypto::Hasher;

use crate::{
    database::tables::{
        blobs::{BlobsTable, BLOBS_TABLE},
        entries::{EntriesTable, Entry, ENTRIES_TABLE},
    },
    error::{Error, Result},
    extractors::{EntryPath, Pubky},
    server::AppState,
};

pub async fn put(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
    mut body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();
    let path = path.as_str().to_string();

    // TODO: can we move this logic to the extractor or a layer
    // to perform this validation?
    let session = state
        .db
        .get_session(cookies, &public_key, &path)?
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    if !path.starts_with("pub/") {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            "Writing to directories other than '/pub/' is forbidden".into(),
        ));
    }

    // TODO: should we forbid paths ending with `/`?

    let mut stream = body.into_data_stream();

    let (tx, rx) = flume::bounded::<Bytes>(1);

    // TODO: refactor Database to clean up this scope.
    let done = tokio::task::spawn_blocking(move || -> Result<()> {
        // TODO: this is a blocking operation, which is ok for small
        // payloads (we have 16 kb limit for now) but later we need
        // to stream this to filesystem, and keep track of any failed
        // writes to GC these files later.

        state.db.put_entry(&public_key, &path, rx);

        Ok(())
    });

    while let Some(next) = stream.next().await {
        let chunk = next?;

        tx.send(chunk);
    }

    drop(tx);
    done.await.expect("join error")?;

    // TODO: return relevant headers, like Etag?

    Ok(())
}

pub async fn get(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
) -> Result<impl IntoResponse> {
    // TODO: check the path, return an error if doesn't start with `/pub/`

    // TODO: Enable streaming

    let public_key = pubky.public_key();

    match state.db.get_blob(public_key, path.as_str()) {
        Err(error) => Err(error)?,
        Ok(Some(bytes)) => Ok(bytes),
        Ok(None) => Err(Error::with_status(StatusCode::NOT_FOUND)),
    }
}
