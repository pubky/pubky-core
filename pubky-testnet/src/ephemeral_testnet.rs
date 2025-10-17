use crate::Testnet;
use http_relay::HttpRelay;
use pubky::Pubky;

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
        me.testnet.create_homeserver().await?;

        Ok(me)
    }

    /// Run a new simple testnet network with a minimal setup.
    pub async fn start_minimal() -> anyhow::Result<Self> {
        let mut me = Self {
            testnet: Testnet::new().await?,
        };
        me.testnet.create_http_relay().await?;
        Ok(me)
    }

    /// Create an additional homeserver with a random keypair
    pub async fn create_random_homeserver(
        &mut self,
    ) -> anyhow::Result<&pubky_homeserver::HomeserverSuite> {
        self.testnet.create_random_homeserver().await
    }

    /// Create a new pubky client builder.
    pub fn client_builder(&self) -> pubky::PubkyHttpClientBuilder {
        self.testnet.client_builder()
    }

    /// Creates a [`pubky::PubkyHttpClient`] pre-configured to use this test network.
    pub fn client(&self) -> Result<pubky::PubkyHttpClient, pubky::BuildError> {
        self.testnet.client()
    }

    /// Creates a [`pubky::Pubky`] SDK facade pre-configured to use this test network.
    ///
    /// This is a convenience method that builds a client from `Self::client_builder`.
    pub fn sdk(&self) -> Result<Pubky, pubky::BuildError> {
        self.testnet.sdk()
    }

    /// Create a new pkarr client builder.
    pub fn pkarr_client_builder(&self) -> pkarr::ClientBuilder {
        self.testnet.pkarr_client_builder()
    }

    /// Get the homeserver in the testnet.
    pub fn homeserver(&self) -> &pubky_homeserver::HomeserverSuite {
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

    #[tokio::test]
    async fn test_homeserver_with_random_keypair() {
        let mut network = EphemeralTestnet::start_minimal().await.unwrap();
        assert!(network.testnet.homeservers.len() == 0);

        let _ = network.create_random_homeserver().await.unwrap();
        let _ = network.create_random_homeserver().await.unwrap();
        assert!(network.testnet.homeservers.len() == 2);

        // The two newly created homeservers must have distinct public keys.
        assert_ne!(
            network.testnet.homeservers[0].public_key(),
            network.testnet.homeservers[1].public_key()
        );
    }
}
