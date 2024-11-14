use wasm_bindgen::prelude::*;

use crate::Client;

mod api;
mod internals;
mod wrappers;

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/pkarr"];

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    /// Create Client with default Settings including default relays
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            pkarr: pkarr::Client::builder().build().unwrap(),
        }
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `["http://localhost:15411/pkarr"]` instead of default relay.
    #[wasm_bindgen]
    pub fn testnet() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            pkarr: pkarr::Client::builder()
                .relays(TESTNET_RELAYS.into_iter().map(|s| s.to_string()).collect())
                .build()
                .unwrap(),
        }
    }
}
