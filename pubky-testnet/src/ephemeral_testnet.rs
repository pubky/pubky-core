use http_relay::HttpRelay;

use crate::Testnet;

/// A simple testnet with random ports assigned for all components.
///
/// - A local DHT with bootstrapping nodes.
/// - http relay.
/// - A homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
/// - An admin server for the homeserver.
pub struct EphemeralTestnet {
    /// Inner flexible testnet.
    pub testnet: Testnet,
}

impl EphemeralTestnet {
    /// Run a new simple testnet.
    pub async fn start() -> anyhow::Result<Self> {
        let mut me = Self {
            testnet: Testnet::new().await?,
        };

        me.testnet.create_http_relay().await?;
        me.testnet.create_homeserver_suite().await?;

        Ok(me)
    }

    /// Create a new pubky client builder.
    pub fn pubky_client_builder(&self) -> pubky::ClientBuilder {
        self.testnet.pubky_client_builder()
    }

    /// Creates a `pubky::Client` pre-configured to use this test network.
    pub fn pubky_client(&self) -> pubky::Client {
        self.testnet.pubky_client()
    }

    /// Create a new pkarr client builder.
    pub fn pkarr_client_builder(&self) -> pkarr::ClientBuilder {
        self.testnet.pkarr_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver_suite(&self) -> &pubky_homeserver::HomeserverSuite {
        self.testnet
            .homeservers
            .first()
            .expect("homeservers should be non-empty")
    }

    /// Get the http relay in the testnet.
    pub fn http_relay(&self) -> &HttpRelay {
        self.testnet
            .http_relays
            .first()
            .expect("http relays should be non-empty")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Test that two testnets can be run in a row.
    /// This is to prevent the case where the testnet is not cleaned up properly.
    /// For example, if the port is not released after the testnet is stopped.
    #[tokio::test]
    async fn test_two_testnet_in_a_row() {
        {
            let _ = EphemeralTestnet::start().await.unwrap();
        }

        {
            let _ = EphemeralTestnet::start().await.unwrap();
        }
    }
}
