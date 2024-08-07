use std::collections::HashMap;

use axum::{
    body::{Body, Bytes},
    debug_handler,
    extract::{Path, Query, State},
    http::{header, Response, StatusCode},
    response::IntoResponse,
    RequestExt, Router,
};
use axum_extra::body::AsyncReadBody;
use futures_util::stream::StreamExt;
use pkarr::PublicKey;
use serde::Deserialize;
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
    let path = path.as_str();

    authorize(&mut state, cookies, &public_key, path)?;
    verify(path)?;

    let mut stream = body.into_data_stream();

    let (tx, rx) = flume::bounded::<Bytes>(1);

    let path = path.to_string();

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
    Query(params): Query<HashMap<String, String>>,
) -> Result<impl IntoResponse> {
    verify(path.as_str());
    let public_key = pubky.public_key();

    let path = path.as_str();

    if path.ends_with('/') {
        let txn = state.db.env.read_txn()?;

        let path = format!("{public_key}/{path}");

        if !state.db.contains_directory(&txn, &path)? {
            return Err(Error::new(
                StatusCode::NOT_FOUND,
                "Directory Not Found".into(),
            ));
        }

        // Handle listing
        let vec = state.db.list(
            &txn,
            &path,
            params.contains_key("reverse"),
            params.get("limit").and_then(|l| l.parse::<u16>().ok()),
            params.get("cursor").map(|cursor| cursor.into()),
            params.contains_key("shallow"),
        )?;

        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(vec.join("\n")))
            .unwrap());
    }

    // TODO: Enable streaming

    match state.db.get_blob(public_key, path) {
        Err(error) => Err(error)?,
        Ok(Some(bytes)) => Ok(Response::builder().body(Body::from(bytes)).unwrap()),
        Ok(None) => Err(Error::new(StatusCode::NOT_FOUND, "File Not Found".into())),
    }
}

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
    mut body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();
    let path = path.as_str();

    authorize(&mut state, cookies, &public_key, path)?;
    verify(path)?;

    let deleted = state.db.delete_entry(&public_key, path)?;

    if !deleted {
        // TODO: if the path ends with `/` return a `CONFLICT` error?
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    }

    // TODO: return relevant headers, like Etag?

    Ok(())
}

fn authorize(
    state: &mut AppState,
    cookies: Cookies,
    public_key: &PublicKey,
    path: &str,
) -> Result<()> {
    // TODO: can we move this logic to the extractor or a layer
    // to perform this validation?
    let session = state
        .db
        .get_session(cookies, public_key, path)?
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    Ok(())
}

fn verify(path: &str) -> Result<()> {
    if !path.starts_with("pub/") {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            "Writing to directories other than '/pub/' is forbidden".into(),
        ));
    }

    // TODO: should we forbid paths ending with `/`?

    Ok(())
}
