//!
//! Code to help with testing Pubky.
//! Only available for use in tests.
//! This module is not included in the crate's public interface.
//!
//! This testnet class is independent of `pubky-testnet` because 
//! it would introduce circular dependencies.
//!

#![cfg_attr(any(), deny(clippy::unwrap_used))]

use anyhow::Result;
use http_relay::HttpRelay;
use pubky_homeserver::Homeserver;

/// A local test network to test the Pubky client.
pub(crate) struct Testnet2 {
    dht: mainline::Testnet,
}

impl Testnet2 {
    /// Run a new testnet.
    pub async fn new() -> Result<Self> {
        let dht = mainline::Testnet::new(3)?;
        let testnet = Self { dht };

        Ok(testnet)
    }

    // === Getters ===

    /// Returns a list of DHT bootstrapping nodes.
    pub fn bootstrap(&self) -> &[String] {
        &self.dht.bootstrap
    }

    // === Public Methods ===

    /// Create a Pubky Homeserver
    pub async fn create_homeserver(&self) -> Result<Homeserver> {
        Homeserver::run_test(&self.dht.bootstrap).await
    }

    /// Create a Pubky Homeserver that requires signup tokens
    pub async fn create_homeserver_with_signup_tokens(&self) -> Result<Homeserver> {
        Homeserver::run_test_with_signup_tokens(&self.dht.bootstrap).await
    }

    /// Create an HTTP Relay
    pub async fn create_http_relay(&self) -> Result<HttpRelay> {
        HttpRelay::builder().run().await
    }

    /// Create a client builder for the testnet.
    pub fn client_builder(&self) -> crate::ClientBuilder {
        let bootstrap = self.bootstrap();

        let mut builder = crate::Client::builder();
        builder.pkarr(|builder| {
            builder
                .bootstrap(bootstrap)
                .no_relays()
        });

        builder
    }
}
