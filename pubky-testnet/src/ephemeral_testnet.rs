use crate::Testnet;
use http_relay::HttpRelay;
use pubky::{Keypair, Pubky};
use pubky_homeserver::{ConfigToml, ConnectionString, MockDataDir};

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

/// Builder for configuring and creating an [`EphemeralTestnet`].
///
/// Provides an API for customizing testnet configuration before creation.
pub struct EphemeralTestnetBuilder {
    postgres_connection_string: Option<ConnectionString>,
    homeserver_config: Option<ConfigToml>,
    homeserver_keypair: Option<Keypair>,
}

impl EphemeralTestnetBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            postgres_connection_string: None,
            homeserver_config: None,
            homeserver_keypair: None,
        }
    }

    /// Set a custom homeserver configuration.
    pub fn config(mut self, config: ConfigToml) -> Self {
        self.homeserver_config = Some(config);
        self
    }

    /// Set a specific keypair for the homeserver.
    pub fn keypair(mut self, keypair: Keypair) -> Self {
        self.homeserver_keypair = Some(keypair);
        self
    }

    /// Set a custom postgres connection string.
    pub fn postgres(mut self, connection_string: ConnectionString) -> Self {
        self.postgres_connection_string = Some(connection_string);
        self
    }

    /// Build and start the testnet with the configured settings.
    pub async fn build(self) -> anyhow::Result<EphemeralTestnet> {
        let mut testnet = if let Some(postgres) = self.postgres_connection_string {
            Testnet::new_with_custom_postgres(postgres).await?
        } else {
            Testnet::new().await?
        };
        testnet.create_http_relay().await?;

        let mut config = self.homeserver_config.unwrap_or_else(ConfigToml::test);

        if let Some(connection_string) = testnet.postgres_connection_string.as_ref() {
            config.general.database_url = connection_string.clone();
        }

        let keypair = self
            .homeserver_keypair
            .unwrap_or_else(|| Keypair::from_secret_key(&[0; 32]));
        let mock_dir = MockDataDir::new(config, Some(keypair))?;
        testnet.create_homeserver_app_with_mock(mock_dir).await?;

        Ok(EphemeralTestnet { testnet })
    }
}

impl Default for EphemeralTestnetBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EphemeralTestnet {
    pub fn builder() -> EphemeralTestnetBuilder {
        EphemeralTestnetBuilder::new()
    }

    /// Run a new simple testnet.
    pub async fn start() -> anyhow::Result<Self> {
        Self::builder().build().await
    }

    /// Run a new simple testnet.
    /// Pass a custom postgres connection string to use for the homeserver.
    pub async fn start_with_custom_postgres(
        postgres_connection_string: ConnectionString,
    ) -> anyhow::Result<Self> {
        Self::builder()
            .postgres(postgres_connection_string)
            .build()
            .await
    }

    /// Run a new simple testnet network with a minimal setup.
    pub async fn start_minimal() -> anyhow::Result<Self> {
        let mut me = Self {
            testnet: Testnet::new().await?,
        };
        me.testnet.create_http_relay().await?;
        Ok(me)
    }

    /// Run a new simple testnet network with a minimal setup.
    /// Pass a custom postgres connection string to use for the homeserver.
    pub async fn start_minimal_with_custom_postgres(
        postgres_connection_string: ConnectionString,
    ) -> anyhow::Result<Self> {
        let mut me = Self {
            testnet: Testnet::new_with_custom_postgres(postgres_connection_string).await?,
        };
        me.testnet.create_http_relay().await?;
        Ok(me)
    }

    /// Create an additional homeserver with a random keypair.
    pub async fn create_random_homeserver(
        &mut self,
        config: Option<ConfigToml>,
    ) -> anyhow::Result<&pubky_homeserver::HomeserverApp> {
        let mut config = config.unwrap_or_else(ConfigToml::test);

        if let Some(connection_string) = self.testnet.postgres_connection_string.as_ref() {
            config.general.database_url = connection_string.clone();
        }

        let mock_dir = MockDataDir::new(config, Some(Keypair::random()))?;
        self.testnet.create_homeserver_app_with_mock(mock_dir).await
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
    pub fn homeserver_app(&self) -> &pubky_homeserver::HomeserverApp {
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

        let _ = network.create_random_homeserver(None).await.unwrap();
        let _ = network.create_random_homeserver(None).await.unwrap();
        assert!(network.testnet.homeservers.len() == 2);

        // The two newly created homeservers must have distinct public keys.
        assert_ne!(
            network.testnet.homeservers[0].public_key(),
            network.testnet.homeservers[1].public_key()
        );
    }

    #[tokio::test]
    async fn test_builder_matches_start() {
        // Ensure builder().build() produces same result as start()
        let network_start = EphemeralTestnet::start().await.unwrap();
        let network_builder = EphemeralTestnet::builder().build().await.unwrap();

        assert_eq!(
            network_start.testnet.homeservers.len(),
            network_builder.testnet.homeservers.len()
        );
        assert_eq!(
            network_start.testnet.http_relays.len(),
            network_builder.testnet.http_relays.len()
        );
    }
}
