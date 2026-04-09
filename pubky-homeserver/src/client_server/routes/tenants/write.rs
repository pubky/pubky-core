use axum::{
    body::Body,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use futures_util::stream::StreamExt;

use crate::{
    client_server::{
        err_if_user_is_invalid::get_user_or_http_error, extractors::PubkyHost, AppState,
    },
    persistence::files::WriteStreamError,
    shared::{
        webdav::{EntryPath, WebDavPathPubAxum},
        HttpResult,
    },
};

pub async fn delete(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key();
    get_user_or_http_error(pubky.public_key(), &mut state.sql_db.pool().into(), false).await?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    state.file_service.delete(&entry_path).await?;
    Ok((StatusCode::NO_CONTENT, ()))
}

pub async fn put(
    State(state): State<AppState>,
    pubky: PubkyHost,
    Path(path): Path<WebDavPathPubAxum>,
    body: Body,
) -> HttpResult<impl IntoResponse> {
    let public_key = pubky.public_key();
    get_user_or_http_error(public_key, &mut state.sql_db.pool().into(), true).await?;
    let entry_path = EntryPath::new(public_key.clone(), path.inner().to_owned());

    // Convert body stream to the format expected by file_service
    let body_stream = body.into_data_stream();
    let converted_stream =
        body_stream.map(|chunk_result| chunk_result.map_err(WriteStreamError::Axum));

    state
        .file_service
        .write_stream(&entry_path, converted_stream)
        .await?;
    Ok((StatusCode::CREATED, ()))
}
