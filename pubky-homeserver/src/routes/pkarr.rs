use std::{collections::HashMap, sync::RwLock};

use axum::{
    body::{Body, Bytes},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Router,
};
use futures_util::stream::StreamExt;
use once_cell::sync::OnceCell;

use pkarr::{PublicKey, SignedPacket};

use crate::{
    error::{Error, Result},
    extractors::Pubky,
};

// TODO: maybe replace after we have local storage of users packets?
static IN_MEMORY: OnceCell<RwLock<HashMap<PublicKey, SignedPacket>>> = OnceCell::new();

/// Pkarr relay, helpful for testing.
///
/// For real productioin, you should use a [production ready
/// relay](https://github.com/pubky/pkarr/server).
pub fn pkarr_router() -> Router {
    Router::new()
        .route("/pkarr/:pubky", put(pkarr_put))
        .route("/pkarr/:pubky", get(pkarr_get))
}

pub async fn pkarr_put(pubky: Pubky, body: Body) -> Result<impl IntoResponse> {
    let mut bytes = Vec::with_capacity(1104);

    let mut stream = body.into_data_stream();

    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?)
    }

    let public_key = pubky.public_key().to_owned();

    let signed_packet = SignedPacket::from_relay_payload(&public_key, &Bytes::from(bytes))?;

    let mut store = IN_MEMORY
        .get()
        .expect("In memory pkarr store is not initialized")
        .write()
        .unwrap();

    store.insert(public_key, signed_packet);

    Ok(())
}

pub async fn pkarr_get(pubky: Pubky) -> Result<impl IntoResponse> {
    let store = IN_MEMORY
        .get()
        .expect("In memory pkarr store is not initialized")
        .read()
        .unwrap();

    if let Some(signed_packet) = store.get(pubky.public_key()) {
        return Ok(signed_packet.to_relay_payload());
    }

    Err(Error::with_status(StatusCode::NOT_FOUND))
}
