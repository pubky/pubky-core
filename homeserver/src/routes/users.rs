use axum::{extract::State, response::IntoResponse};
use bytes::Bytes;
use http::StatusCode;
use pkarr::SignedPacket;

use crate::error::{Error, Result};
use crate::server::AppState;

pub async fn register(State(state): State<AppState>, body: Bytes) -> Result<impl IntoResponse> {
    // TODO: define a better error?

    let signed_packet =
        SignedPacket::from_bytes(&body).map_err(|_| Error::with_status(StatusCode::BAD_REQUEST))?;

    for x in signed_packet.resource_records("_pk") {
        match &x.rdata {
            pkarr::dns::rdata::RData::TXT(txt) => {
                let attributes = txt.attributes();
                let home = attributes.get("home");

                if Some(&Some(state.public_key.to_string())) == home {
                    dbg!(home);

                    // TOOD: add user and return its user_id?
                    return Ok("Registered user");
                }
            }
            _ => {}
        }
    }

    Err(Error::new(StatusCode::BAD_REQUEST, Some("Does not match!")))
}
