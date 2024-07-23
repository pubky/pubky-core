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
    State(state): State<AppState>,
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

        let mut wtxn = state.db.env.write_txn()?;
        let blobs: BlobsTable = state
            .db
            .env
            .open_database(&wtxn, Some(BLOBS_TABLE))?
            .expect("Blobs table already created");

        let entries: EntriesTable = state
            .db
            .env
            .open_database(&wtxn, Some(ENTRIES_TABLE))?
            .expect("Entries table already created");

        let mut hasher = Hasher::new();
        let mut bytes = vec![];
        let mut length = 0;

        while let Ok(chunk) = rx.recv() {
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
            length += chunk.len();
        }

        let hash = hasher.finalize();

        blobs.put(&mut wtxn, hash.as_bytes(), &bytes)?;

        let mut entry = Entry::new();

        entry.set_content_hash(hash);
        entry.set_content_length(length);

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

        entries.put(&mut wtxn, &key, &entry.serialize());

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
    State(state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
) -> Result<impl IntoResponse> {
    // TODO: check the path, return an error if doesn't start with `/pub/`

    // TODO: Enable streaming

    let public_key = pubky.public_key();

    let mut rtxn = state.db.env.read_txn()?;

    let entries: EntriesTable = state
        .db
        .env
        .open_database(&rtxn, Some(ENTRIES_TABLE))?
        .expect("Entries table already created");

    let blobs: BlobsTable = state
        .db
        .env
        .open_database(&rtxn, Some(BLOBS_TABLE))?
        .expect("Blobs table already created");

    let mut count = 0;

    for x in entries.iter(&rtxn)? {
        count += 1
    }

    return Err(Error::new(StatusCode::NOT_FOUND, count.to_string().into()));

    let mut key = vec![];
    key.extend_from_slice(public_key.as_bytes());
    key.extend_from_slice(path.as_bytes());

    if let Some(bytes) = entries.get(&rtxn, &key)? {
        let entry = Entry::deserialize(bytes)?;

        if let Some(blob) = blobs.get(&rtxn, entry.content_hash())? {
            return Ok(blob.to_vec());
        };
    };

    Err(Error::new(StatusCode::NOT_FOUND, path.0.into()))
}
