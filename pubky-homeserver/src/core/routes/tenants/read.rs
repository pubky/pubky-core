use crate::persistence::files::FileIoError;
use crate::persistence::lmdb::tables::files::Entry;
use crate::shared::{HttpError, HttpResult};
use crate::{
    core::{
        err_if_user_is_invalid::err_if_user_is_invalid,
        error::{Error, Result},
        extractors::{ListQueryParams, PubkyHost},
        AppState,
    },
    shared::webdav::{EntryPath, WebDavPathPubAxum},
};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Response, StatusCode},
    response::IntoResponse,
};
use httpdate::HttpDate;
use std::str::FromStr;

pub async fn head(
    State(state): State<AppState>,
    pubky: PubkyHost,
    headers: HeaderMap,
    Path(path): Path<WebDavPathPubAxum>,
) -> HttpResult<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let entry_path = EntryPath::new(pubky.public_key().clone(), path.inner().clone());

    let entry = state.file_service.get_info(&entry_path).await?;
    get_entry(headers, entry, None)
}

#[axum::debug_handler]
pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
    params: ListQueryParams,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key().clone();
    let dav_path = path.0;
    let entry_path = EntryPath::new(public_key.clone(), dav_path.inner().clone());
    if entry_path.path().is_directory() {
        return list(state, &entry_path, params);
    }

    let (entry, stream) = state.file_service.get_info_and_stream(&entry_path).await?;
    let body_stream = Body::from_stream(stream);
    get_entry(headers, entry, Some(body_stream))
}

pub fn list(
    state: AppState,
    entry_path: &EntryPath,
    params: ListQueryParams,
) -> HttpResult<Response<Body>> {
    let txn = state.db.env.read_txn()?;

    if !state.db.contains_directory(&txn, entry_path)? {
        return Err(HttpError::new(StatusCode::NOT_FOUND, Some("Directory Not Found")));
    }

    // Handle listing
    let vec = state.db.list_entries(
        &txn,
        entry_path,
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

pub fn get_entry(headers: HeaderMap, entry: Entry, body: Option<Body>) -> HttpResult<Response<Body>> {
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
    }
    Ok(response)
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
    use axum::http::{header, StatusCode};
    use pkarr::Keypair;
    use pubky_common::{auth::AuthToken, capabilities::Capability};

    use crate::{app_context::AppContext, core::HomeserverCore};

    pub async fn create_root_user(
        server: &axum_test::TestServer,
        keypair: &Keypair,
    ) -> anyhow::Result<String> {
        let auth_token = AuthToken::sign(keypair, vec![Capability::root()]);
        let body_bytes: axum::body::Bytes = auth_token.serialize().into();
        let response = server
            .post("/signup")
            .add_header("host", keypair.public_key().to_string())
            .bytes(body_bytes)
            .expect_success()
            .await;

        let header_value = response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|h| h.to_str().ok())
            .expect("should return a set-cookie header")
            .to_string();

        Ok(header_value)
    }

    #[tokio::test]
    async fn if_last_modified() {
        let context = AppContext::test();
        let router = HomeserverCore::create_router(&context);
        let server = axum_test::TestServer::new(router).unwrap();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let cookie = create_root_user(&server, &keypair)
            .await
            .unwrap()
            .to_string();

        let data = vec![1_u8, 2, 3, 4, 5];

        server
            .put("/pub/foo")
            .add_header("host", public_key.to_string())
            .add_header(header::COOKIE, cookie)
            .bytes(data.into())
            .expect_success()
            .await;

        let response = server
            .get("/pub/foo")
            .add_header("host", public_key.to_string())
            .expect_success()
            .await;

        let response = server
            .get("/pub/foo")
            .add_header("host", public_key.to_string())
            .add_header(
                header::IF_MODIFIED_SINCE,
                response.headers().get(header::LAST_MODIFIED).unwrap(),
            )
            .await;

        response.assert_status(StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn if_none_match() {
        let context = AppContext::test();
        let router = HomeserverCore::create_router(&context);
        let server = axum_test::TestServer::new(router).unwrap();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();

        let cookie = create_root_user(&server, &keypair)
            .await
            .unwrap()
            .to_string();

        let data = vec![1_u8, 2, 3, 4, 5];

        server
            .put("/pub/foo")
            .add_header("host", public_key.to_string())
            .add_header(header::COOKIE, cookie)
            .bytes(data.into())
            .expect_success()
            .await;

        let response = server
            .get("/pub/foo")
            .add_header("host", public_key.to_string())
            .expect_success()
            .await;

        let response = server
            .get("/pub/foo")
            .add_header("host", public_key.to_string())
            .add_header(
                header::IF_NONE_MATCH,
                response.headers().get(header::ETAG).unwrap(),
            )
            .await;

        response.assert_status(StatusCode::NOT_MODIFIED);
    }
}
