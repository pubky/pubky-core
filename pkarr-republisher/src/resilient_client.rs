use pkarr::{PublicKey, SignedPacket};

use crate::{
    PublishError, PublishInfo, Publisher, PublisherSettings, RepublishError, RepublishInfo,
    Republisher, RepublisherSettings, RetrySettings,
};

#[derive(Debug, thiserror::Error)]
pub enum ResilientClientBuilderError {
    #[error(transparent)]
    BuildError(#[from] pkarr::errors::BuildError),
}

/// Simple pkarr client that focuses on resilience
/// compared to the regular client that might experience
/// unreliability due to the underlying UDP connection.
#[derive(Debug, Clone)]
pub struct ResilientClient {
    client: pkarr::Client,
    retry_settings: RetrySettings,
}

impl ResilientClient {
    pub fn new() -> Result<Self, ResilientClientBuilderError> {
        let client = pkarr::Client::builder().build()?;
        Self::new_with_client(client, RetrySettings::default())
    }

    pub fn new_with_client(
        client: pkarr::Client,
        retry_settings: RetrySettings,
    ) -> Result<Self, ResilientClientBuilderError> {
        Ok(Self {
            client,
            retry_settings,
        })
    }

    /// Publishes a pkarr packet with retries.
    pub async fn publish(
        &self,
        packet: SignedPacket,
    ) -> Result<PublishInfo, PublishError> {
        let mut settings = PublisherSettings::default();
        settings.pkarr_client(self.client.clone());
        settings.retry_settings(self.retry_settings.clone());
        let publisher = Publisher::new_with_settings(packet, settings)
            .expect("infallible because pkarr client provided.");
        publisher.publish().await
    }

    /// Republishes a pkarr packet with retries.
    pub async fn republish(
        &self,
        public_key: PublicKey,
    ) -> Result<RepublishInfo, RepublishError> {
        let mut settings = RepublisherSettings::default();
        settings.pkarr_client(self.client.clone());
        settings.retry_settings(self.retry_settings.clone());
        let publisher = Republisher::new_with_settings(public_key, settings)
            .expect("infallible because pkarr client provided.");
        publisher.republish().await
    }
}
