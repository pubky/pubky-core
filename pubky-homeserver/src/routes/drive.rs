use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    RequestExt, Router,
};
use futures_util::stream::StreamExt;

use tracing::debug;

use pubky_common::crypto::Hasher;

use crate::{
    database::tables::blobs::{BlobsTable, BLOBS_TABLE},
    error::{Error, Result},
    extractors::Pubky,
    server::AppState,
};

pub async fn put(
    State(state): State<AppState>,
    pubky: Pubky,
    // Path(key): Path<String>,
    mut body: Body,
) -> Result<impl IntoResponse> {
    let mut stream = body.into_data_stream();

    let (tx, rx) = flume::bounded::<Bytes>(1);

    // Offload the write transaction to a blocking task
    let done = tokio::task::spawn_blocking(move || {
        // TODO: this is a blocking operation, which is ok for small
        // payloads (we have 16 kb limit for now) but later we need
        // to stream this to filesystem, and keep track of any failed
        // writes to GC these files later.

        let mut wtxn = state.db.env.write_txn().unwrap();
        let blobs: BlobsTable = state
            .db
            .env
            .open_database(&wtxn, Some(BLOBS_TABLE))
            .unwrap()
            .expect("Blobs table already created");

        let hasher = Hasher::new();

        while let Ok(chunk) = rx.recv() {
            dbg!(chunk);
        }
    });

    while let Some(next) = stream.next().await {
        let chunk = next
            .map_err(|err| Error::new(StatusCode::INTERNAL_SERVER_ERROR, Some(err.to_string())))?;

        tx.send(chunk);
    }

    let _ = done.await;

    Ok("Pubky drive...".to_string())
}
