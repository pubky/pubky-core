use wasm_bindgen::prelude::*;

use crate::PubkyClient;

mod api;
mod internals;
mod wrappers;

impl Default for PubkyClient {
    fn default() -> Self {
        Self::new()
    }
}

static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/pkarr"];

#[wasm_bindgen]
impl PubkyClient {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            pkarr: pkarr::Client::builder().build().unwrap(),
            pkarr_relays: vec!["https://relay.pkarr.org".to_string()],
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
            pkarr_relays: TESTNET_RELAYS.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Set Pkarr relays used for publishing and resolving Pkarr packets.
    ///
    /// By default, [PubkyClient] will use `["https://relay.pkarr.org"]`
    #[wasm_bindgen(js_name = "setPkarrRelays")]
    pub fn set_pkarr_relays(mut self, relays: Vec<String>) -> Self {
        self.pkarr_relays = relays;
        self
    }

    // Read the set of pkarr relays used by this client.
    #[wasm_bindgen(js_name = "getPkarrRelays")]
    pub fn get_pkarr_relays(&self) -> Vec<String> {
        self.pkarr_relays.clone()
    }
}
