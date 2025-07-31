use std::{num::NonZeroU64, time::Duration};

use pubky::ClientConfig;
use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*; // Import ClientConfig directly

use super::js_result::JsResult;
use super::wasm_http_client::WasmHttpClient;

static TESTNET_RELAY_PORT: &str = "15411";

// JS style config objects remain the same.
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PkarrConfig {
    #[tsify(optional)]
    pub(crate) relays: Option<Vec<String>>,
    #[tsify(optional)]
    pub(crate) request_timeout: Option<NonZeroU64>,
}
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyClientConfig {
    #[tsify(optional)]
    pub(crate) pkarr: Option<PkarrConfig>,
    #[tsify(optional)]
    pub(crate) user_max_record_age: Option<NonZeroU64>,
}

/// The WASM-exposed Pubky client.
/// This is a wrapper around the generic `pubky::Client`, configured with the
/// `WasmHttpClient` for web environments.
#[wasm_bindgen]
pub struct Client {
    pub(crate) inner: pubky::BaseClient<WasmHttpClient>,
}

#[wasm_bindgen]
impl Client {
    /// Create a new Pubky Client with an optional configuration.
    #[wasm_bindgen(constructor)]
    pub fn new(config_opt: Option<PubkyClientConfig>) -> JsResult<Self> {
        // 1. Create a config object using the correct platform-agnostic constructor.
        let mut config = ClientConfig::new();
        let mut max_record_age: Option<Duration> = None;

        // 2. Apply JS config if provided.
        if let Some(js_config) = config_opt {
            if let Some(pkarr_conf) = js_config.pkarr {
                // We combine both pkarr configurations into a single call.
                config.pkarr(|pkarr_builder| {
                    if let Some(relays) = &pkarr_conf.relays {
                        pkarr_builder.no_relays(); // Remove default pkarr relays
                        if let Err(e) = pkarr_builder.relays(relays) {
                            log::error!("Failed to set relays: {}", e);
                        }
                    }
                    if let Some(timeout) = pkarr_conf.request_timeout {
                        pkarr_builder.request_timeout(Duration::from_millis(timeout.get()));
                    }
                    pkarr_builder
                });
            }
            if let Some(age) = js_config.user_max_record_age {
                let duration = Duration::from_secs(age.get());
                max_record_age = Some(duration);
                config.max_record_age(duration);
            }
        }

        // 3. Build the platform-agnostic components.
        let pkarr_client = config
            .build_pkarr_client()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // 4. Create the WASM-specific HTTP client.
        let http_client = WasmHttpClient::new(pkarr_client.clone(), None);

        // 5. Assemble the final generic client.
        let inner = pubky::BaseClient::new(http_client, pkarr_client, max_record_age);

        Ok(Self { inner })
    }

    /// Create a client configured for local testing.
    #[wasm_bindgen]
    pub fn testnet(host: Option<String>) -> JsResult<Self> {
        let hostname = host.unwrap_or_else(|| "localhost".to_string());
        let testnet_relay = format!("http://{}:{}/", hostname, TESTNET_RELAY_PORT);

        let mut config = ClientConfig::new();
        config.pkarr(|builder| {
            builder
                .no_relays()
                .relays(&[testnet_relay.as_str()])
                .expect("testnet relays should be valid urls")
        });

        let pkarr_client = config
            .build_pkarr_client()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let http_client = WasmHttpClient::new(pkarr_client.clone(), Some(hostname));

        let inner = pubky::BaseClient::new(http_client, pkarr_client, None);

        Ok(Self { inner })
    }
}
