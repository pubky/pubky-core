use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use httpdate::HttpDate;
use pkarr::PublicKey;
use std::str::FromStr;

use crate::{
    core::AppState,
    database::tables::entries::Entry,
    error::{Error, Result},
    extractors::{EntryPath, ListQueryParams, Pubky},
};

use super::verify;

pub async fn head(
    State(state): State<AppState>,
    pubky: Pubky,
    headers: HeaderMap,
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

pub async fn list_root(
    State(state): State<AppState>,
    pubky: Pubky,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    list(state, pubky.public_key(), "pub/", params)
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    pubky: Pubky,
    path: EntryPath,
    params: ListQueryParams,
) -> Result<impl IntoResponse> {
    verify(&path)?;

    let public_key = pubky.public_key().clone();

    if path.as_str().ends_with('/') {
        return list(state, &public_key, &path, params);
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

pub fn list(
    state: AppState,
    public_key: &PublicKey,
    path: &str,
    params: ListQueryParams,
) -> Result<Response<Body>> {
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

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(vec.join("\n")))?)
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
    use axum::{
        body::Body,
        http::{header, Method, Request, StatusCode},
    };
    use pkarr::Keypair;

    use crate::core::HomeserverCore;

    #[tokio::test]
    async fn if_last_modified() {
        let mut server = HomeserverCore::test().unwrap();

        let public_key = Keypair::random().public_key();
        let cookie = server.create_user(&public_key).unwrap();
        let cookie = cookie.to_string();

        let url = format!("/{public_key}/pub/foo");

        let data = vec![1_u8, 2, 3, 4, 5];

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::PUT)
                    .header(header::COOKIE, cookie)
                    .body(Body::from(data))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::GET)
                    .header(
                        header::IF_MODIFIED_SINCE,
                        response.headers().get(header::LAST_MODIFIED).unwrap(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn if_none_match() {
        let mut server = HomeserverCore::test().unwrap();

        let public_key = Keypair::random().public_key();
        let cookie = server.create_user(&public_key).unwrap();
        let cookie = cookie.to_string();

        let url = format!("/{public_key}/pub/foo");

        let data = vec![1_u8, 2, 3, 4, 5];

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::PUT)
                    .header(header::COOKIE, cookie)
                    .body(Body::from(data))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::GET)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let response = server
            .call(
                Request::builder()
                    .uri(&url)
                    .method(Method::GET)
                    .header(
                        header::IF_NONE_MATCH,
                        response.headers().get(header::ETAG).unwrap(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }
}
