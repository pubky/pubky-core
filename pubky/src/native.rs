use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use pkarr::{mainline::Testnet, PublicKey};
use reqwest::{cookie::CookieStore, header::HeaderValue, Response};

use crate::Client;

mod api;
mod http;
mod internals;

static DEFAULT_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

#[derive(Debug, Default)]
pub struct Settings {
    pkarr_settings: pkarr::Settings,
}

impl Settings {
    /// Set Pkarr client [pkarr::Settings].
    pub fn pkarr_settings(mut self, settings: pkarr::Settings) -> Self {
        self.pkarr_settings = settings;
        self
    }

    /// Sets the following:
    /// - Pkarr client's DHT bootstrap nodes = `testnet` bootstrap nodes.
    /// - Pkarr client's resolvers           = `testnet` bootstrap nodes.
    /// - Pkarr client's DHT request timout  = 500 milliseconds. (unless in CI, then it is left as default 2000)
    pub fn testnet(mut self, testnet: &Testnet) -> Self {
        let bootstrap = testnet.bootstrap.clone();

        let mut dht_settings = pkarr::mainline::Settings::default().bootstrap(&bootstrap);

        if std::env::var("CI").is_err() {
            dht_settings = dht_settings.request_timeout(Duration::from_millis(500));
        }

        self.pkarr_settings = self
            .pkarr_settings
            .dht_settings(dht_settings)
            .resolvers(Some(bootstrap));

        self
    }

    /// Build [Client]
    pub fn build(self) -> Result<Client, std::io::Error> {
        let pkarr = pkarr::Client::new(self.pkarr_settings)?;

        let cookie_store = Arc::new(CookieJar::default());

        let http = reqwest::Client::builder()
            .cookie_provider(cookie_store.clone())
            // TODO: use persistent cookie jar
            .dns_resolver(Arc::new(pkarr.clone()))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .expect("config expected to not error");

        Ok(Client {
            cookie_store,
            http,
            pkarr,
        })
    }
}

impl Client {
    /// Create a new [Client] with default [Settings]
    pub fn new() -> Result<Self, std::io::Error> {
        Self::builder().build()
    }

    /// Returns a builder to edit settings before creating [Client].
    pub fn builder() -> Settings {
        Settings::default()
    }

    /// Create a client connected to the local network
    /// with the bootstrapping node: `localhost:6881`
    pub fn testnet() -> Result<Self, std::io::Error> {
        Self::builder()
            .testnet(&Testnet {
                bootstrap: vec!["localhost:6881".to_string()],
                nodes: vec![],
            })
            .build()
    }

    #[cfg(test)]
    /// Alias to `pubky::Client::builder().testnet(testnet).build().unwrap()`
    pub(crate) fn test(testnet: &Testnet) -> Client {
        Client::builder().testnet(testnet).build().unwrap()
    }
}

#[derive(Default)]
pub struct CookieJar {
    pubky_sessions: RwLock<HashMap<String, String>>,
    normal_jar: RwLock<cookie_store::CookieStore>,
}

impl CookieJar {
    pub(crate) fn store_session_after_signup(&self, response: &Response, pubky: PublicKey) {
        for (header_name, header_value) in response.headers() {
            if header_name == "set-cookie" {
                if header_value.as_ref().starts_with(b"session_id=") {
                    if let Ok(Ok(cookie)) =
                        std::str::from_utf8(header_value.as_bytes()).map(cookie::Cookie::parse)
                    {
                        if cookie.name() == "session_id" {
                            let domain = format!("_pubky.{pubky}");
                            tracing::debug!(?cookie, "Storing coookie after signup");

                            self.pubky_sessions
                                .write()
                                .unwrap()
                                .insert(domain, cookie.value().to_string());
                        }
                    };
                }
            }
        }
    }

    pub(crate) fn delete_session_after_signout(&self, pubky: &PublicKey) {
        self.pubky_sessions
            .write()
            .unwrap()
            .remove(&format!("_pubky.{pubky}"));
    }
}

impl CookieStore for CookieJar {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &url::Url) {
        let iter = cookie_headers.filter_map(|val| {
            val.to_str()
                .ok()
                .and_then(|s| cookie::Cookie::parse(s.to_owned()).ok())
        });

        self.normal_jar
            .write()
            .unwrap()
            .store_response_cookies(iter, url);
    }

    fn cookies(&self, url: &url::Url) -> Option<HeaderValue> {
        let s = self
            .normal_jar
            .read()
            .unwrap()
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");

        if s.is_empty() {
            return self
                .pubky_sessions
                .read()
                .unwrap()
                .get(url.host_str().unwrap())
                .and_then(|secret| {
                    Some(HeaderValue::try_from(format!("session_id={secret}")).unwrap())
                });
        }

        HeaderValue::from_maybe_shared(bytes::Bytes::from(s)).ok()
    }
}
