// js/src/constructor.rs
use std::{num::NonZeroU64, time::Duration};

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use super::js_result::JsResult;
use crate::js_error::{PubkyErrorName, PubkyJsError};

static TESTNET_RELAY_PORT: &str = "15411";

// ------------------------------------------------------------------------------------------------
// JS style config objects for the client.
// ------------------------------------------------------------------------------------------------

/// Pkarr Config
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PkarrConfig {
    /// The list of relays to access the DHT with.
    #[tsify(optional)]
    pub(crate) relays: Option<Vec<String>>,
    /// The timeout for DHT requests in milliseconds.
    /// Default is 2000ms.
    #[tsify(optional)]
    pub(crate) request_timeout: Option<NonZeroU64>,
}

/// Pubky Client Config
#[derive(Tsify, Serialize, Deserialize, Debug)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PubkyClientConfig {
    /// Configuration on how to access pkarr packets on the mainline DHT.
    #[tsify(optional)]
    pub(crate) pkarr: Option<PkarrConfig>,
    // NOTE: user_max_record_age belonged to the old client — removed here.
}

#[wasm_bindgen]
pub struct Client(pub(crate) pubky::PubkyHttpClient);

impl Default for Client {
    fn default() -> Self {
        Self::new(None).expect("default constructor should be infallible")
    }
}

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    /// Create a new Pubky HTTP client with an optional configuration.
    pub fn new(config_opt: Option<PubkyClientConfig>) -> JsResult<Self> {
        let mut builder = pubky::PubkyHttpClient::builder();

        if let Some(config) = config_opt {
            if let Some(pkarr) = config.pkarr {
                // Relays
                if let Some(relays) = pkarr.relays {
                    let mut relay_set_error: Option<String> = None;
                    builder.pkarr(|p| {
                        p.no_relays();
                        if let Err(e) = p.relays(&relays) {
                            relay_set_error = Some(e.to_string());
                        }
                        p
                    });
                    if let Some(msg) = relay_set_error {
                        return Err(PubkyJsError::new(PubkyErrorName::InvalidInput, msg));
                    }
                }
                // Timeout
                if let Some(timeout_ms) = pkarr.request_timeout {
                    builder.pkarr(|p| {
                        p.request_timeout(Duration::from_millis(timeout_ms.get()));
                        p
                    });
                }
            }
        }

        Ok(Self(builder.build()?))
    }

    /// Create a client configured for **local testnet**.
    /// - PKARR relays → `http://<host>:15411/`
    /// - WASM endpoint mapping via `.testnet_host(host)`
    #[wasm_bindgen]
    pub fn testnet(host: Option<String>) -> Self {
        let hostname = host.unwrap_or_else(|| "localhost".to_string());
        let relay = format!("http://{}:{}/", hostname, TESTNET_RELAY_PORT);

        let mut builder = pubky::PubkyHttpClient::builder();
        builder.pkarr(|p| p.relays(&[relay.as_str()]).expect("valid testnet relay"));
        builder.testnet_host(hostname); // no-op on native, active on WASM

        let client = builder.build().expect("testnet build should be infallible");
        Self(client)
    }
}
