use pkarr::PublicKey;

use super::republisher::{RepublishError, RepublishInfo, Republisher, RepublisherSettings};

#[derive(Debug, thiserror::Error)]
pub enum ResilientClientBuilderError {
    #[error("pkarr client was built without DHT and is only using relays. This is not supported.")]
    DhtNotEnabled,
    #[error(transparent)]
    BuildError(#[from] pkarr::errors::BuildError),
}

/// Simple pkarr client that focuses on resilience
/// and verification compared to the regular client that
/// might experience inreliability due to the underlying UDP connection.
///
/// This client requires a pkarr client that was built with the `dht` feature.
/// Relays only are not supported.
#[derive(Debug, Clone)]
pub struct ResilientClient {
    client: pkarr::Client,
    settings: RepublisherSettings,
}

impl ResilientClient {
    pub fn new(
        client: pkarr::Client,
        settings: RepublisherSettings,
    ) -> Result<Self, ResilientClientBuilderError> {
        if client.dht().is_none() {
            return Err(ResilientClientBuilderError::DhtNotEnabled);
        }
        Ok(Self { client, settings })
    }

    /// Republishes a pkarr packet with retries. Verifies it's been stored correctly.
    pub async fn republish(&self, public_key: &PublicKey) -> Result<RepublishInfo, RepublishError> {
        Republisher::new_with_settings(self.client.clone(), self.settings.clone())
            .republish(public_key)
            .await
    }
}
