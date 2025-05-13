use wasm_bindgen::prelude::*;

pub mod api {
    pub mod auth;
    pub mod http;
    pub mod public;
    pub mod recovery_file;
}

pub mod wrappers {
    pub mod keys;
    pub mod session;
}

static TESTNET_RELAYS: [&str; 1] = ["http://localhost:15411/"];

#[wasm_bindgen]
pub struct Client(crate::NativeClient);

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Client {
    #[wasm_bindgen(constructor)]
    /// Create Client with default Settings including default relays
    pub fn new() -> Self {
        Self(
            crate::NativeClient::builder()
                .build()
                .expect("building a default NativeClient should be infallible"),
        )
    }

    /// Create a client with with configurations appropriate for local testing:
    /// - set Pkarr relays to `["http://localhost:15411"]` instead of default relay.
    /// - transform `pubky://<pkarr public key>` to `http://<pkarr public key` instead of `https:`
    ///     and read the homeserver HTTP port from the [reserved service parameter key](pubky_common::constants::reserved_param_keys::HTTP_PORT)
    #[wasm_bindgen]
    pub fn testnet() -> Self {
        let mut builder = crate::NativeClient::builder();

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
