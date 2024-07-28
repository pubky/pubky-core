use wasm_bindgen::prelude::*;

pub mod auth;
pub mod keys;
pub mod pkarr;

use crate::PubkyClient;

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            // pkarr: pkarr::PkarrRelayClient::default(),
        }
    }
}
