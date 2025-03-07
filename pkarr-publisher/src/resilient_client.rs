use std::num::NonZeroU8;

use pkarr::{mainline::async_dht::AsyncDht, PublicKey, SignedPacket};

use crate::{
    count_key_on_dht, PublishError, PublishInfo, Publisher, PublisherSettings, RepublishError,
    RepublishInfo, Republisher, RepublisherSettings, RetrySettings,
};

/// Simple pkarr client that focuses on resilience
/// and verification compared to the regular client that
/// might experience inreliability due to the underlying UDP connection.
pub struct ResilientClient {
    client: pkarr::Client,
    dht: AsyncDht,
    retry_settings: RetrySettings,
}

impl ResilientClient {
    pub fn new() -> Result<Self, pkarr::errors::BuildError> {
        let client = pkarr::Client::builder().build()?;
        Ok(Self::new_with_client(client, RetrySettings::new()))
    }

    pub fn new_with_client(client: pkarr::Client, retry_settings: RetrySettings) -> Self {
        let dht = client.dht().unwrap().as_async();
        Self {
            client,
            dht,
            retry_settings,
        }
    }

    /// Publishes a pkarr packet with retries. Verifies it's been stored correctly.
    pub async fn publish(
        &self,
        public_key: PublicKey,
        packet: SignedPacket,
        min_sufficient_node_publish_count: Option<NonZeroU8>,
    ) -> Result<PublishInfo, PublishError> {
        let mut settings = PublisherSettings::new();
        settings.pkarr_client(self.client.clone());
        settings.retry_settings(self.retry_settings.clone());
        if let Some(count) = min_sufficient_node_publish_count {
            settings.min_sufficient_node_publish_count = count;
        };
        let publisher = Publisher::new_with_settings(public_key, packet, settings)
            .expect("infalliable because pkarr client provided.");
        publisher.publish().await
    }

    /// Republishes a pkarr packet with retries. Verifies it's been stored correctly.
    pub async fn republish(
        &self,
        public_key: PublicKey,
        min_sufficient_node_publish_count: Option<NonZeroU8>,
    ) -> Result<RepublishInfo, RepublishError> {
        let mut settings = RepublisherSettings::new();
        settings.pkarr_client(self.client.clone());
        if let Some(count) = min_sufficient_node_publish_count {
            settings.min_sufficient_node_publish_count = count;
        };
        settings.retry_settings(self.retry_settings.clone());
        let publisher = Republisher::new_with_settings(public_key, settings)
            .expect("infalliable because pkarr client provided.");
        publisher.republish().await
    }

    /// Counts the number of nodes the public key has been stored on.
    pub async fn verify_node_count(&self, public_key: &PublicKey) -> usize {
        count_key_on_dht(&public_key, &self.dht).await
    }
}
