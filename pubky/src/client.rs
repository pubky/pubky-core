mod auth;
mod pkarr;

use std::{collections::HashMap, fmt::format, time::Duration};

use ureq::{Agent, Response};
use url::Url;

use crate::error::{Error, Result};

use pkarr::{DhtSettings, PkarrClient, Settings, Testnet};

#[derive(Debug, Clone)]
pub struct PubkyClient {
    agent: Agent,
    pkarr: PkarrClient,
}

impl PubkyClient {
    pub fn new() -> Self {
        Self {
            agent: Agent::new(),
            pkarr: PkarrClient::new(Default::default()).unwrap(),
        }
    }

    pub fn test(testnet: &Testnet) -> Self {
        Self {
            agent: Agent::new(),
            pkarr: PkarrClient::new(Settings {
                dht: DhtSettings {
                    request_timeout: Some(Duration::from_millis(10)),
                    bootstrap: Some(testnet.bootstrap.to_owned()),
                    ..DhtSettings::default()
                },
                ..Settings::default()
            })
            .unwrap(),
        }
    }

    // === Public Methods ===

    // === Private Methods ===

    fn request(&self, method: HttpMethod, url: &Url) -> ureq::Request {
        self.agent.request_url(method.into(), url)
    }
}

impl Default for PubkyClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Delete,
}

impl From<HttpMethod> for &str {
    fn from(value: HttpMethod) -> Self {
        match value {
            HttpMethod::Get => "GET",
            HttpMethod::Put => "PUT",
            HttpMethod::Post => "POST",
            HttpMethod::Delete => "DELETE",
        }
    }
}
