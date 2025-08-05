use std::{num::NonZeroU64, time::Duration};

use serde::{Deserialize, Serialize};
use tsify::Tsify;
use wasm_bindgen::prelude::*;

use crate::js_error::JsError;

use super::js_result::JsResult;

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
    /// The maximum age of a record in seconds.
    /// If the user pkarr record is older than this, it will be automatically refreshed.
    #[tsify(optional)]
    pub(crate) user_max_record_age: Option<NonZeroU64>,
}

// ------------------------------------------------------------------------------------------------
// JS Client constructor
// ------------------------------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Client(pub(crate) pubky::Client);

impl Default for Client {
    fn default() -> Self {
        Self::new(None).expect("No config constructor should be infallible")
    }
}

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    /// Create a new Pubky Client with an optional configuration.
    pub fn new(config_opt: Option<PubkyClientConfig>) -> JsResult<Self> {
        let mut builder = pubky::Client::builder();

        let config = match config_opt {
            Some(config) => config,
            None => {
                return Ok(Self(
                    builder
                        .build()
                        .expect("building a default NativeClient should be infallible"),
                ));
            }
        };

        if let Some(pkarr) = config.pkarr {
            // Set pkarr relays
            if let Some(relays) = pkarr.relays {
                let mut relay_set_error: Option<String> = None;
                builder.pkarr(|pkarr_builder| {
                    pkarr_builder.no_relays();
                    if let Err(e) = pkarr_builder.relays(&relays) {
                        relay_set_error = Some(e.to_string());
                    }
                    pkarr_builder
                });

                if let Some(error_message) = relay_set_error {
                    return Err(JsError {
                        name: "InvalidRelayUrl".to_string(),
                        message: error_message,
                    });
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

        let native_client = builder.build()?;
        Ok(Self(native_client))
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `http://<host>:15411` (defaults to `localhost`).
    /// - transform `pubky://<pkarr public key>` to `http://<host>` instead of `https:`
    ///   and read the homeserver HTTP port from the PKarr record.
    #[wasm_bindgen]
    pub fn testnet(host: Option<String>) -> Self {
        let hostname = host.unwrap_or_else(|| "localhost".to_string());
        let testnet_relay = format!("http://{}:{}/", hostname, TESTNET_RELAY_PORT);

        let mut builder = pubky::Client::builder();

        builder.pkarr(|builder| {
            builder
                .relays(&[testnet_relay.as_str()])
                .expect("testnet relays are valid urls")
        });

        // Store the testnet hostname for URL transformations.
        builder.testnet_host(hostname);

        let client = builder.build().expect("testnet build should be infallible");

        Self(client)
    }
}
