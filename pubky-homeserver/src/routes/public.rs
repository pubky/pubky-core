use axum::{
    body::Body,
    debug_handler,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use futures_util::stream::StreamExt;
use httpdate::HttpDate;
use pkarr::PublicKey;
use std::{io::Write, str::FromStr};
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

    let (_entry, sse_event) = entry_writer.commit()?;

    if let Some(sse_event) = sse_event {
        let _ = state.events.send(sse_event);
    }

    // TODO: return relevant headers, like Etag?

    Ok(())
}

#[debug_handler]
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
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

    get_entry(
        headers,
        entry_rx.recv_async().await?,
        Some(Body::from_stream(chunks_rx.into_stream())),
    )
}

pub async fn head(
    State(state): State<AppState>,
    headers: HeaderMap,
    pubky: Pubky,
    path: EntryPath,
) -> Result<impl IntoResponse> {
    verify(path.as_str())?;

    let rtxn = state.db.env.read_txn()?;

    get_entry(
        headers,
        state
            .db
            .get_entry(&rtxn, pubky.public_key(), path.as_str())?,
        None,
    )
}

pub fn get_entry(
    headers: HeaderMap,
    entry: Option<Entry>,
    body: Option<Body>,
) -> Result<Response<Body>> {
    if let Some(entry) = entry {
        // TODO: Enable seek API (range requests)
        // TODO: Gzip? or brotli?

        let mut response = HeaderMap::from(&entry).into_response();

        // Handle IF_MODIFIED_SINCE
        if let Some(condition_http_date) = headers
            .get(header::IF_MODIFIED_SINCE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| HttpDate::from_str(s).ok())
        {
            let entry_http_date: HttpDate = entry.timestamp().to_owned().into();

            if condition_http_date >= entry_http_date {
                *response.status_mut() = StatusCode::NOT_MODIFIED;
            }
        };

        // Handle IF_NONE_MATCH
        if let Some(str) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|h| h.to_str().ok())
        {
            let etag = format!("\"{}\"", entry.content_hash());
            if str
                .trim()
                .split(',')
                .collect::<Vec<_>>()
                .contains(&etag.as_str())
            {
                *response.status_mut() = StatusCode::NOT_MODIFIED;
            };
        }

        if let Some(body) = body {
            *response.body_mut() = body;
        };

        Ok(response)
    } else {
        Err(Error::with_status(StatusCode::NOT_FOUND))?
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
    let (deleted, sse_event) = state.db.delete_entry(&public_key, path)?;

    if !deleted {
        // TODO: if the path ends with `/` return a `CONFLICT` error?
        return Err(Error::with_status(StatusCode::NOT_FOUND));
    };

    if let Some(event) = sse_event {
        let _ = state.events.send(event);
    }

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

#[cfg(test)]
mod tests {
    use axum::http::header;
    use pkarr::{mainline::Testnet, Keypair};
    use reqwest::{self, Method, StatusCode};

    use crate::Homeserver;

    #[tokio::test]
    async fn if_last_modified() -> anyhow::Result<()> {
        let testnet = Testnet::new(3);
        let mut server = Homeserver::start_test(&testnet).await?;

        let public_key = Keypair::random().public_key();

        let data = &[1, 2, 3, 4, 5];

        server
            .database_mut()
            .write_entry(&public_key, "pub/foo")?
            .update(data)?
            .commit()?;

        let client = reqwest::Client::builder().build()?;

        let url = format!("http://localhost:{}/{public_key}/pub/foo", server.port());

        let response = client.request(Method::GET, &url).send().await?;

        let response = client
            .request(Method::GET, &url)
            .header(
                header::IF_MODIFIED_SINCE,
                response.headers().get(header::LAST_MODIFIED).unwrap(),
            )
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        let response = client
            .request(Method::HEAD, &url)
            .header(
                header::IF_MODIFIED_SINCE,
                response.headers().get(header::LAST_MODIFIED).unwrap(),
            )
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        Ok(())
    }

    #[tokio::test]
    async fn if_none_match() -> anyhow::Result<()> {
        let testnet = Testnet::new(3);
        let mut server = Homeserver::start_test(&testnet).await?;

        let public_key = Keypair::random().public_key();

        let data = &[1, 2, 3, 4, 5];

        server
            .database_mut()
            .write_entry(&public_key, "pub/foo")?
            .update(data)?
            .commit()?;

        let client = reqwest::Client::builder().build()?;

        let url = format!("http://localhost:{}/{public_key}/pub/foo", server.port());

        let response = client.request(Method::GET, &url).send().await?;

        let response = client
            .request(Method::GET, &url)
            .header(
                header::IF_NONE_MATCH,
                response.headers().get(header::ETAG).unwrap(),
            )
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        let response = client
            .request(Method::HEAD, &url)
            .header(
                header::IF_NONE_MATCH,
                response.headers().get(header::ETAG).unwrap(),
            )
            .send()
            .await?;

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);

        Ok(())
    }
}
