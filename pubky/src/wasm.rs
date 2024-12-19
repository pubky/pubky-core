use wasm_bindgen::prelude::*;

use crate::Client;

mod api;
mod http;
mod wrappers;

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/"];

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    /// Create Client with default Settings including default relays
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            pkarr: pkarr::Client::builder().build().unwrap(),
            testnet: false,
        }
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `["http://localhost:15411"]` instead of default relay.
    #[wasm_bindgen]
    pub fn testnet() -> Self {
        Self {
            http: reqwest::Client::builder().build().unwrap(),
            pkarr: pkarr::Client::builder()
                .relays(
                    TESTNET_RELAYS
                        .into_iter()
                        .map(|s| url::Url::parse(s).expect("TESTNET_RELAYS should be valid urls"))
                        .collect(),
                )
                .build()
                .unwrap(),
            testnet: true,
        }
    }
}

#[wasm_bindgen(js_name = "setLogLevel")]
pub fn set_log_level(level: &str) -> Result<(), JsValue> {
    let level = match level.to_lowercase().as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        "debug" => log::Level::Debug,
        "trace" => log::Level::Trace,
        _ => return Err(JsValue::from_str("Invalid log level")),
    };

    console_log::init_with_level(level).map_err(|e| JsValue::from_str(&e.to_string()))?;
    log::info!("Log level set to: {}", level);
    Ok(())
}
