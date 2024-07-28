pub mod auth;
pub mod pkarr;
pub mod public;

use std::time::Duration;

use ::pkarr::{
    mainline::dht::{DhtSettings, Testnet},
    PkarrClient, PkarrClientAsync, Settings,
};

use crate::PubkyClient;

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

impl PubkyClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            #[cfg(not(target_arch = "wasm32"))]
            pkarr: PkarrClient::new(Default::default()).unwrap().as_async(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn test(testnet: &Testnet) -> Self {
        Self {
            http: reqwest::Client::builder()
                .cookie_store(true)
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            pkarr: PkarrClient::new(Settings {
                dht: DhtSettings {
                    request_timeout: Some(Duration::from_millis(10)),
                    bootstrap: Some(testnet.bootstrap.to_owned()),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })
            .unwrap()
            .as_async(),
        }
    }
}

impl Default for PubkyClient {
    fn default() -> Self {
        Self::new()
    }
}
