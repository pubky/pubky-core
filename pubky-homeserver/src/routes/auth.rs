use axum::{extract::State, response::IntoResponse};
use bytes::Bytes;

use crate::{error::Result, extractors::Pubky, server::AppState};

pub async fn signup(
    State(state): State<AppState>,
    pubky: Pubky,
    body: Bytes,
) -> Result<impl IntoResponse> {
    state.verifier.verify(&body, pubky.public_key())?;

    // TODO: store account in database.

    Ok(())
}
