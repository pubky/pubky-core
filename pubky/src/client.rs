mod auth;
mod pkarr;
mod public;

use std::{collections::HashMap, fmt::format, time::Duration};

use ::pkarr::PkarrClientAsync;
use url::Url;

use pkarr::{DhtSettings, PkarrClient, Settings, Testnet};

use crate::error::{Error, Result};

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Clone)]
pub struct PubkyClient {
    http: reqwest::Client,
    pkarr: PkarrClientAsync,
}

impl PubkyClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(DEFAULT_USER_AGENT)
                .build()
                .unwrap(),
            pkarr: PkarrClient::new(Default::default()).unwrap().as_async(),
        }
    }

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
