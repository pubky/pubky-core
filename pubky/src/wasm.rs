use wasm_bindgen::prelude::*;

pub mod auth;
pub mod keys;
pub mod pkarr;

#[wasm_bindgen]
pub struct PubkyClient {
    pub(crate) pkarr: pkarr::PkarrRelayClient,
}
