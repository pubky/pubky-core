use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use futures_util::stream::StreamExt;
use pkarr::PublicKey;
use std::io::Write;
use tower_cookies::Cookies;

use crate::{
    database::tables::entries::Entry,
    error::{Error, Result},
    extractors::{EntryPath, ListQueryParams, Pubky},
    server::AppState,
};

pub async fn put(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
    body: Body,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();
    let path = path.as_str().to_string();

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

pub async fn get(
    State(state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    verify(path.as_str())?;
    let public_key = pubky.public_key().clone();
    let path = path.as_str().to_string();

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
            params.reverse,
            params.limit,
            params.cursor,
            params.shallow,
        )?;

        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from(vec.join("\n")))?);
    }

    let (entry_tx, entry_rx) = flume::bounded::<Option<Entry>>(1);
    let (chunks_tx, chunks_rx) = flume::unbounded::<std::result::Result<Vec<u8>, heed::Error>>();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let rtxn = state.db.env.read_txn()?;

        let option = state.db.get_entry(&rtxn, &public_key, &path)?;

        if let Some(entry) = option {
            let iter = entry.read_content(&state.db, &rtxn)?;

            entry_tx.send(Some(entry))?;

            for next in iter {
                chunks_tx.send(next.map(|b| b.to_vec()))?;
            }
        };

        entry_tx.send(None)?;

        Ok(())
    });

    if let Some(entry) = entry_rx.recv_async().await? {
        // TODO: add HEAD endpoint
        // TODO: Enable seek API (range requests)
        // TODO: Gzip? or brotli?

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, entry.content_length())
            .header(header::CONTENT_TYPE, entry.content_type())
            .header(
                header::ETAG,
                format!("\"{}\"", entry.content_hash().to_hex()),
            )
            .body(Body::from_stream(chunks_rx.into_stream()))
            .unwrap())
    } else {
        Err(Error::with_status(StatusCode::NOT_FOUND))?
    }
}

pub async fn head(
    State(state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
) -> Result<impl IntoResponse> {
    verify(path.as_str())?;

    let rtxn = state.db.env.read_txn()?;

    match state
        .db
        .get_entry(&rtxn, pubky.public_key(), path.as_str())?
        .as_ref()
        .map(HeaderMap::from)
    {
        Some(headers) => Ok(headers),
        None => Err(Error::with_status(StatusCode::NOT_FOUND)),
    }
}

pub async fn delete(
    State(mut state): State<AppState>,
    pubky: Pubky,
    path: EntryPath,
    cookies: Cookies,
) -> Result<impl IntoResponse> {
    let public_key = pubky.public_key().clone();
    let path = path.as_str();

    authorize(&mut state, cookies, &public_key, path)?;
    verify(path)?;

    // TODO: should we wrap this with `tokio::task::spawn_blocking` in case it takes too long?
    let deleted = state.db.delete_entry(&public_key, path)?;

    if !deleted {
        // TODO: if the path ends with `/` return a `CONFLICT` error?
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    };

    Ok(())
}

/// Authorize write (PUT or DELETE) for Public paths.
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
        .get_session(cookies, public_key)?
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    if session.pubky() == public_key
        && session.capabilities().iter().any(|cap| {
            path.starts_with(&cap.scope[1..])
                && cap
                    .actions
                    .contains(&pubky_common::capabilities::Action::Write)
        })
    {
        return Ok(());
    }

    Err(Error::with_status(StatusCode::FORBIDDEN))
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

impl From<&Entry> for HeaderMap {
    fn from(entry: &Entry) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_LENGTH, entry.content_length().into());
        headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(&entry.timestamp().format_http_date())
                .expect("http date is valid header value"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            // TODO: when setting content type from user input, we should validate it as a HeaderValue
            entry
                .content_type()
                .try_into()
                .or(HeaderValue::from_str(""))
                .expect("valid header value"),
        );
        headers.insert(
            header::ETAG,
            format!("\"{}\"", entry.content_hash())
                .try_into()
                .expect("hex string is valid"),
        );

        headers
    }
}
