use http_relay::HttpRelay;

use crate::FlexibleTestnet;

/// A simple testnet with random ports assigned for all components.
///
/// - A local DHT with bootstrapping nodes.
/// - pkarr relay.
/// - http relay.
/// - A homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`.
/// - An admin server for the homeserver.
pub struct SimpleTestnet {
    /// Inner flexible testnet.
    pub flexible_testnet: FlexibleTestnet,
}

impl SimpleTestnet {
    /// Run a new simple testnet.
    pub async fn run() -> anyhow::Result<Self> {
        let mut me = Self {
            flexible_testnet: FlexibleTestnet::new().await?,
        };

        me.flexible_testnet.create_http_relay().await?;
        me.flexible_testnet.create_homeserver_suite().await?;

        Ok(me)
    }

    /// Create a new pubky client builder.
    pub fn pubky_client_builder(&self) -> pubky::ClientBuilder {
        self.flexible_testnet.pubky_client_builder()
    }

    /// Create a new pkarr client builder.
    pub fn pkarr_client_builder(&self) -> pkarr::ClientBuilder {
        self.flexible_testnet.pkarr_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver_suite(&self) -> &pubky_homeserver::HomeserverSuite {
        self.flexible_testnet
            .homeservers
            .first()
            .expect("homeservers should be non-empty")
    }

    /// Get the http relay in the testnet.
    pub fn http_relay(&self) -> &HttpRelay {
        self.flexible_testnet
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
            let _ = SimpleTestnet::run().await.unwrap();
        }

        {
            let _ = SimpleTestnet::run().await.unwrap();
        }
    }
}
