use std::num::NonZeroU8;

use pkarr::PublicKey;

use super::publisher::RetrySettings;
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
    retry_settings: RetrySettings,
}

impl ResilientClient {
    pub fn new_with_client(
        client: pkarr::Client,
        retry_settings: RetrySettings,
    ) -> Result<Self, ResilientClientBuilderError> {
        if client.dht().is_none() {
            return Err(ResilientClientBuilderError::DhtNotEnabled);
        }
        Ok(Self {
            client,
            retry_settings,
        })
    }

    /// Republishes a pkarr packet with retries. Verifies it's been stored correctly.
    pub async fn republish(
        &self,
        public_key: PublicKey,
        min_sufficient_node_publish_count: Option<NonZeroU8>,
    ) -> Result<RepublishInfo, RepublishError> {
        let mut settings = RepublisherSettings::default();
        settings.pkarr_client(self.client.clone());
        if let Some(count) = min_sufficient_node_publish_count {
            settings.min_sufficient_node_publish_count = count;
        };
        settings.retry_settings(self.retry_settings.clone());
        let publisher = Republisher::new_with_settings(public_key, settings)
            .expect("infallible because pkarr client provided.");
        publisher.republish().await
    }
}
