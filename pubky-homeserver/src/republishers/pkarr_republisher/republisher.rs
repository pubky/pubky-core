//!
//! Republishes a single public key.
//!
use pkarr::PublicKey;
use pkarr::SignedPacket;
use std::{num::NonZeroU8, sync::Arc};

use super::publisher::{PublishError, Publisher, PublisherSettings};
use super::retrying_republisher::RetrySettings;

#[derive(thiserror::Error, Debug, Clone)]
pub enum RepublishError {
    #[error("The packet can't be resolved on the DHT and therefore can't be republished.")]
    Missing,
    #[error(transparent)]
    PublishFailed(#[from] PublishError),
}

impl RepublishError {
    pub(super) fn is_recoverable(&self) -> bool {
        // TODO: We do not retry on Missing because in the next pkarr version it
        // returns Missing when it is truly missing and not just an error.
        matches!(self, Self::PublishFailed(_))
    }
}

pub type RepublishCondition = dyn Fn(&SignedPacket) -> bool + Send + Sync;

/// Settings for creating a republisher
#[derive(Clone)]
pub struct RepublisherSettings {
    pub(crate) min_sufficient_node_publish_count: NonZeroU8,
    pub(super) retry_settings: RetrySettings,
    pub(crate) republish_condition: Option<Arc<RepublishCondition>>,
}

impl std::fmt::Debug for RepublisherSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepublisherSettings")
            .field(
                "min_sufficient_node_publish_count",
                &self.min_sufficient_node_publish_count,
            )
            .field("retry_settings", &self.retry_settings)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl RepublisherSettings {
    /// Set a closure that determines whether a packet should be republished
    pub fn republish_condition<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&SignedPacket) -> bool + Send + Sync + 'static,
    {
        self.republish_condition = Some(Arc::new(f));
        self
    }

    /// Set the minimum sufficient number of nodes a key needs to be stored in
    /// to be considered a success
    pub fn min_sufficient_node_publish_count(&mut self, count: NonZeroU8) -> &mut Self {
        self.min_sufficient_node_publish_count = count;
        self
    }
}

impl Default for RepublisherSettings {
    fn default() -> Self {
        Self {
            min_sufficient_node_publish_count: NonZeroU8::new(10).expect("Should always be > 0"),
            retry_settings: RetrySettings::default(),
            republish_condition: None,
        }
    }
}

/// Tries to republish a single key once.
#[derive(Clone)]
pub struct Republisher {
    client: pkarr::Client,
    settings: RepublisherSettings,
}

impl std::fmt::Debug for Republisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Republisher")
            .field("client", &self.client)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl Republisher {
    pub fn new_with_settings(client: pkarr::Client, settings: RepublisherSettings) -> Self {
        Self { client, settings }
    }

    /// Republish a single public key.
    pub async fn republish(&self, public_key: &PublicKey) -> Result<usize, RepublishError> {
        let packet = self
            .client
            .resolve_most_recent(public_key)
            .await
            .ok_or(RepublishError::Missing)?;

        if self
            .settings
            .republish_condition
            .as_ref()
            .is_some_and(|condition| !condition(&packet))
        {
            return Ok(0);
        }

        let mut settings = PublisherSettings::default();
        settings
            .pkarr_client(self.client.clone())
            .min_sufficient_node_publish_count(self.settings.min_sufficient_node_publish_count);
        let publisher = Publisher::new_with_settings(packet, settings)
            .expect("infallible because pkarr client provided");

        publisher.publish().await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use super::{RepublishError, Republisher, RepublisherSettings};
    use pkarr::{dns::Name, Keypair, PublicKey};

    async fn publish_sample_packets(client: &pkarr::Client) -> PublicKey {
        let key = Keypair::random();

        let packet = pkarr::SignedPacketBuilder::default()
            .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
            .build(&key)
            .unwrap();
        client
            .publish(&packet, None)
            .await
            .expect("to be published");

        key.public_key()
    }

    #[tokio::test]
    async fn single_key_republish_success() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder
            .no_default_network()
            .bootstrap(&dht.bootstrap)
            .no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings.min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Republisher::new_with_settings(pkarr_client, settings);
        let res = publisher.republish(&public_key).await;

        assert_eq!(res.unwrap(), 1);
    }

    #[tokio::test]
    async fn single_key_republish_missing() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings.min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Republisher::new_with_settings(pkarr_client, settings);
        let res = publisher.republish(&public_key).await;

        assert!(matches!(res, Err(RepublishError::Missing)));
    }

    #[tokio::test]
    async fn republish_with_condition_fail() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
            // Only republish if the packet has a TTL greater than 300
            .republish_condition(|_| false);

        let publisher = Republisher::new_with_settings(pkarr_client, settings);
        let res = publisher.republish(&public_key).await;

        assert_eq!(res.unwrap(), 0);
    }

    #[tokio::test]
    async fn republish_with_condition_success() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
            // Only republish if the packet has a TTL greater than 300
            .republish_condition(|_| true);

        let publisher = Republisher::new_with_settings(pkarr_client, settings);
        let res = publisher.republish(&public_key).await;

        assert_eq!(res.unwrap(), 1);
    }
}
