use crate::persistence::lmdb::tables::entries::Entry;
use crate::shared::{HttpError, HttpResult};
use crate::{
    client_server::{
        err_if_user_is_invalid::err_if_user_is_invalid,
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
    Path(path): Path<WebDavPathPubAxum>,
) -> HttpResult<impl IntoResponse> {
    err_if_user_is_invalid(pubky.public_key(), &state.db, false)?;
    let entry_path = EntryPath::new(pubky.public_key().clone(), path.inner().clone());

    let entry = state.file_service.get_info(&entry_path).await?;
    let response = entry.to_response_headers().into_response();
    Ok(response)
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

    let entry = state.file_service.get_info(&entry_path).await?;

    // Handle IF_MODIFIED_SINCE
    if let Some(condition_http_date) = headers
        .get(header::IF_MODIFIED_SINCE)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| HttpDate::from_str(s).ok())
    {
        let entry_http_date: HttpDate = entry.timestamp().to_owned().into();
        if condition_http_date >= entry_http_date {
            return not_modified_response(&entry);
        }
    };

    // Handle IF_NONE_MATCH
    if let Some(request_etag) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|h| h.to_str().ok())
    {
        let current_etag = format!("\"{}\"", entry.content_hash());
        if request_etag
            .trim()
            .split(',')
            .collect::<Vec<_>>()
            .contains(&current_etag.as_str())
        {
            return not_modified_response(&entry);
        };
    }

    let stream = state.file_service.get_stream(&entry_path).await?;
    let body_stream = Body::from_stream(stream);
    let mut response = entry.to_response_headers().into_response();
    *response.body_mut() = body_stream;
    Ok(response)
}

pub fn list(
    state: AppState,
    entry_path: &EntryPath,
    params: ListQueryParams,
) -> HttpResult<Response<Body>> {
    let txn = state.db.env.read_txn()?;

    if !state.db.contains_directory(&txn, entry_path)? {
        return Err(HttpError::new_with_message(
            StatusCode::NOT_FOUND,
            "Directory Not Found",
        ));
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

/// Creates the Not Modified response based on the entry data.
fn not_modified_response(entry: &Entry) -> HttpResult<Response<Body>> {
    Ok(Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::ETAG, format!("\"{}\"", entry.content_hash()))
        .header(header::LAST_MODIFIED, entry.timestamp().format_http_date())
        .body(Body::empty())?)
}

impl Entry {
    pub fn to_response_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_LENGTH, self.content_length().into());
        headers.insert(
            header::LAST_MODIFIED,
            HeaderValue::from_str(&self.timestamp().format_http_date())
                .expect("http date is valid header value"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            self.content_type()
                .try_into()
                .or(HeaderValue::from_str(""))
                .expect("valid header value"),
        );
        headers.insert(
            header::ETAG,
            format!("\"{}\"", self.content_hash())
                .try_into()
                .expect("hex string is valid"),
        );

        headers
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{header, StatusCode};
    use axum::Router;
    use axum_test::TestServer;
    use pkarr::{Keypair, PublicKey};
    use pubky_common::{auth::AuthToken, capabilities::Capability};

    use crate::{app_context::AppContext, client_server::ClientServer};

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

    pub async fn create_environment(
    ) -> anyhow::Result<(AppContext, Router, TestServer, PublicKey, String)> {
        let context = AppContext::test();
        let router = ClientServer::create_router(&context);
        let server = axum_test::TestServer::new(router.clone()).unwrap();

        let keypair = Keypair::random();
        let public_key = keypair.public_key();
        let cookie = create_root_user(&server, &keypair)
            .await
            .unwrap()
            .to_string();

        Ok((context, router, server, public_key, cookie))
    }

    #[tokio::test]
    async fn if_last_modified() {
        let (_context, _router, server, public_key, cookie) = create_environment().await.unwrap();

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
        let (_, _, server, public_key, cookie) = create_environment().await.unwrap();

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

    #[tokio::test]
    async fn test_content_with_magic_bytes() {
        let (_, _, server, public_key, cookie) = create_environment().await.unwrap();

        let data = vec![0x89_u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

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
            .await;

        response.assert_header(header::CONTENT_TYPE, "image/png");
    }

    #[tokio::test]
    async fn test_content_by_extension() {
        let (_, _, server, public_key, cookie) = create_environment().await.unwrap();

        let data = vec![108, 111, 114, 101, 109, 32, 105, 112, 115, 117, 109];

        server
            .put("/pub/text.txt")
            .add_header("host", public_key.to_string())
            .add_header(header::COOKIE, cookie)
            .bytes(data.into())
            .expect_success()
            .await;

        let response = server
            .get("/pub/text.txt")
            .add_header("host", public_key.to_string())
            .await;

        response.assert_header(header::CONTENT_TYPE, "text/plain");
    }
}
