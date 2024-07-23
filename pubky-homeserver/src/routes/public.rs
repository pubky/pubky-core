use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    RequestExt, Router,
};
use axum_extra::body::AsyncReadBody;
use futures_util::stream::StreamExt;

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
    mut body: Body,
) -> Result<impl IntoResponse> {
    // TODO: return an error if path does not start with '/pub/'

    let mut stream = body.into_data_stream();

    let (tx, rx) = flume::bounded::<Bytes>(1);

    // TODO: refactor Database to clean up this scope.
    let done = tokio::task::spawn_blocking(move || -> Result<()> {
        // TODO: this is a blocking operation, which is ok for small
        // payloads (we have 16 kb limit for now) but later we need
        // to stream this to filesystem, and keep track of any failed
        // writes to GC these files later.

        let public_key = pubky.public_key();

        // TODO: Authorize

        state.db.put_entry(public_key, path.as_str(), rx);

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
