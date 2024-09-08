use axum::{
    body::{Body, Bytes},
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, put},
    Router,
};
use futures_util::stream::StreamExt;

use pkarr::SignedPacket;

use crate::{
    error::{Error, Result},
    extractors::Pubky,
    server::AppState,
};

/// Pkarr relay, helpful for testing.
///
/// For real productioin, you should use a [production ready
/// relay](https://github.com/pubky/pkarr/server).
pub fn pkarr_router(state: AppState) -> Router {
    Router::new()
        .route("/:pubky", put(pkarr_put))
        .route("/:pubky", get(pkarr_get))
        .with_state(state)
}

pub async fn pkarr_put(
    State(state): State<AppState>,
    pubky: Pubky,
    body: Body,
) -> Result<impl IntoResponse> {
    let mut bytes = Vec::with_capacity(1104);

    let mut stream = body.into_data_stream();

    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?)
    }

    let public_key = pubky.public_key().to_owned();

    let signed_packet = SignedPacket::from_relay_payload(&public_key, &Bytes::from(bytes))?;

    state.pkarr_client.publish(&signed_packet).await?;

    Ok(())
}

pub async fn pkarr_get(State(state): State<AppState>, pubky: Pubky) -> Result<impl IntoResponse> {
    if let Some(signed_packet) = state.pkarr_client.resolve(pubky.public_key()).await? {
        return Ok(signed_packet.to_relay_payload());
    }

    Err(Error::with_status(StatusCode::NOT_FOUND))
}
