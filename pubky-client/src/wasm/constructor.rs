use std::{num::NonZeroU64, time::Duration};

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use super::js_result::JsResult;

static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/"];

// ------------------------------------------------------------------------------------------------
// JS style config objects for the client.
// ------------------------------------------------------------------------------------------------

/// Pkarr Config
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PkarrConfig {
    /// The list of relays to access the DHT with.
    pub(crate) relays: Option<Vec<String>>,
    /// The timeout for DHT requests in milliseconds.
    /// Default is 2000ms.
    pub(crate) request_timeout: Option<NonZeroU64>,
}

/// Pubky WasmClient Config
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyClientConfig {
    /// Configuration on how to access pkarr packets on the mainline DHT.
    pub(crate) pkarr: Option<PkarrConfig>,
    /// The maximum age of a record in seconds.
    /// If the user pkarr record is older than this, it will be automatically refreshed.
    pub(crate) user_max_record_age: Option<NonZeroU64>,
}

// ------------------------------------------------------------------------------------------------
// JS WasmClient constructor
// ------------------------------------------------------------------------------------------------

#[wasm_bindgen]
pub struct WasmClient(pub(crate) crate::Client);

impl Default for WasmClient {
    fn default() -> Self {
        Self::new(None).expect("No config constructor should be infallible")
    }
}

#[wasm_bindgen]
impl WasmClient {
    #[wasm_bindgen(constructor)]
    /// Create a new Pubky WasmClient with an optional configuration.
    pub fn new(config_opt: Option<PubkyClientConfig>) -> JsResult<Self> {
        let mut builder = crate::Client::builder();

        let config = match config_opt {
            Some(config) => config,
            None => {
                return Ok(Self(
                    builder
                        .build()
                        .expect("building a default native Client should be infallible"),
                ));
            }
        };

        if let Some(pkarr) = config.pkarr {
            // Set pkarr relays
            if let Some(relays) = pkarr.relays {
                let mut relay_set_error: Option<JsValue> = None;
                builder.pkarr(|pkarr_builder| {
                    pkarr_builder.no_relays(); // Remove default pkarr config
                    if let Err(e) = pkarr_builder.relays(&relays) {
                        relay_set_error =
                            Some(JsValue::from_str(&format!("Failed to set relays. {}", e)));
                    }
                    pkarr_builder
                });
                if let Some(e) = relay_set_error {
                    return Err(e);
                }
            }
            // Set pkarr timeout
            if let Some(timeout) = pkarr.request_timeout {
                builder.pkarr(|pkarr_builder| {
                    pkarr_builder.request_timeout(Duration::from_millis(timeout.get()));
                    pkarr_builder
                });
            }
        }

        // Set homeserver max record age
        if let Some(max_record_age) = config.user_max_record_age {
            builder.max_record_age(Duration::from_secs(max_record_age.get()));
        }

        let native_client = builder
            .build()
            .map_err(|e| JsValue::from_str(&format!("Failed to build client. {}", e)))?;
        Ok(Self(native_client))
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `["http://localhost:15411"]` instead of default relay.
    /// - transform `pubky://<pkarr public key>` to `http://<pkarr public key` instead of `https:`
    ///     and read the homeserver HTTP port from the [reserved service parameter key](pubky_common::constants::reserved_param_keys::HTTP_PORT)
    #[wasm_bindgen]
    pub fn testnet() -> Self {
        let mut builder = crate::Client::builder();

        builder.pkarr(|builder| {
            builder
                .relays(&TESTNET_RELAYS)
                .expect("testnet relays are valid urls")
        });

        let mut client = builder.build().expect("testnet build should be infallible");

        client.testnet = true;

        Self(client)
    }
}
