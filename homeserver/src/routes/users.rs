use axum::{extract::State, response::IntoResponse};
use bytes::Bytes;
use http::StatusCode;
use pkarr::SignedPacket;

use pk_common::homeserver::auth::AuthnSignature;

use crate::error::{Error, Result};
use crate::server::AppState;

pub async fn register(State(state): State<AppState>, body: Bytes) -> Result<impl IntoResponse> {
    // TODO: define a better error?

    let signed_packet =
        SignedPacket::from_bytes(&body).map_err(|_| Error::with_status(StatusCode::BAD_REQUEST))?;

    if state.public_key == pk_common::pkarr::homeserver(&signed_packet).unwrap() {
        // TODO: publish and republish
        state.pkarr_client.publish(&signed_packet).await.unwrap();
        dbg!("published", signed_packet.public_key());

        // TODO: add user and return its user_id?
        return Ok("Registered user");
    }

    Err(Error::new(StatusCode::BAD_REQUEST, Some("Does not match!")))
}

pub async fn authn(State(state): State<AppState>, body: Bytes) -> Result<impl IntoResponse> {
    let session = AuthnSignature::verify(&body, &state.public_key);

    dbg!(&session);

    Ok(())
}
