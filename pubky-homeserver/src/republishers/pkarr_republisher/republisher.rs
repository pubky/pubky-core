//!
//! Republishes a single public key.
//!
use pkarr::{PublicKey, SignedPacket};
use std::{num::NonZeroU8, sync::Arc};

use super::publisher::{PublishError, Publisher, PublisherSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepublishOutcome {
    Published(usize),
    Skipped,
    Missing,
    InvalidSignedPacket,
}

pub(super) type RepublishCondition = dyn Fn(&SignedPacket) -> bool + Send + Sync;

/// Settings for creating a republisher
#[derive(Clone)]
pub(super) struct RepublisherSettings {
    pub(super) min_sufficient_node_publish_count: NonZeroU8,
    pub(super) republish_condition: Option<Arc<RepublishCondition>>,
}

impl std::fmt::Debug for RepublisherSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepublisherSettings")
            .field(
                "min_sufficient_node_publish_count",
                &self.min_sufficient_node_publish_count,
            )
            .field(
                "has_republish_condition",
                &self.republish_condition.is_some(),
            )
            .finish()
    }
}

impl Default for RepublisherSettings {
    fn default() -> Self {
        Self {
            min_sufficient_node_publish_count: NonZeroU8::new(10).expect("Should always be > 0"),
            republish_condition: None,
        }
    }
}

/// Tries to republish a single key once.
#[derive(Debug)]
pub(super) struct Republisher {
    client: pkarr::Client,
    settings: RepublisherSettings,
}

impl Republisher {
    pub(super) fn new(client: pkarr::Client, settings: RepublisherSettings) -> Self {
        Self { client, settings }
    }

    /// Republish a single public key.
    pub(super) async fn republish(
        &self,
        public_key: &PublicKey,
    ) -> Result<RepublishOutcome, PublishError> {
        let Some(packet) = self.client.resolve_most_recent(public_key).await else {
            return Ok(RepublishOutcome::Missing);
        };
        if packet.public_key() != *public_key {
            return Ok(RepublishOutcome::InvalidSignedPacket);
        }

        let should_republish = self
            .settings
            .republish_condition
            .as_ref()
            .is_none_or(|condition| condition(&packet));
        if !should_republish {
            return Ok(RepublishOutcome::Skipped);
        }

        let mut settings = PublisherSettings::default();
        settings
            .pkarr_client(self.client.clone())
            .min_sufficient_node_publish_count(self.settings.min_sufficient_node_publish_count);
        let publisher = Publisher::new_with_settings(packet, settings)
            .expect("infallible because pkarr client provided");

        publisher.publish().await.map(RepublishOutcome::Published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::{dns::Name, Keypair};

    async fn publish_sample_packet(client: &pkarr::Client) -> PublicKey {
        let key = Keypair::random();

        let packet = pkarr::SignedPacketBuilder::default()
            .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
            .build(&key)
            .unwrap();
        client
            .publish(&packet, None)
            .await
            .expect("sample packet should publish");

        key.public_key()
    }

    #[tokio::test]
    async fn republish_returns_published_for_resolved_packet() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder
            .no_default_network()
            .bootstrap(&dht.bootstrap)
            .no_relays();
        let pkarr_client = pkarr_builder.build().unwrap();
        let public_key = publish_sample_packet(&pkarr_client).await;

        let settings = RepublisherSettings {
            min_sufficient_node_publish_count: NonZeroU8::MIN,
            ..RepublisherSettings::default()
        };
        let republisher = Republisher::new(pkarr_client, settings);
        let outcome = republisher.republish(&public_key).await.unwrap();

        assert_eq!(outcome, RepublishOutcome::Published(1));
    }

    #[tokio::test]
    async fn republish_returns_missing_for_unknown_key() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.build().unwrap();
        let public_key = Keypair::random().public_key();

        let settings = RepublisherSettings {
            min_sufficient_node_publish_count: NonZeroU8::MIN,
            ..RepublisherSettings::default()
        };
        let republisher = Republisher::new(pkarr_client, settings);
        let outcome = republisher.republish(&public_key).await.unwrap();

        assert_eq!(outcome, RepublishOutcome::Missing);
    }

    #[tokio::test]
    async fn republish_returns_skipped_when_condition_rejects_packet() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.build().unwrap();
        let public_key = publish_sample_packet(&pkarr_client).await;

        let settings = RepublisherSettings {
            min_sufficient_node_publish_count: NonZeroU8::MIN,
            republish_condition: Some(Arc::new(|_| false)),
        };

        let republisher = Republisher::new(pkarr_client, settings);
        let outcome = republisher.republish(&public_key).await.unwrap();

        assert_eq!(outcome, RepublishOutcome::Skipped);
    }

    #[tokio::test]
    async fn republish_returns_published_when_condition_accepts_packet() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.build().unwrap();
        let public_key = publish_sample_packet(&pkarr_client).await;

        let settings = RepublisherSettings {
            min_sufficient_node_publish_count: NonZeroU8::MIN,
            republish_condition: Some(Arc::new(|_| true)),
        };

        let republisher = Republisher::new(pkarr_client, settings);
        let outcome = republisher.republish(&public_key).await.unwrap();

        assert_eq!(outcome, RepublishOutcome::Published(1));
    }

    #[tokio::test]
    async fn republish_returns_publish_error_when_insufficiently_published() {
        let dht = pkarr::mainline::Testnet::builder(1).build().unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder
            .no_default_network()
            .bootstrap(&dht.bootstrap)
            .no_relays();
        let pkarr_client = pkarr_builder.build().unwrap();
        let public_key = publish_sample_packet(&pkarr_client).await;

        let settings = RepublisherSettings {
            min_sufficient_node_publish_count: NonZeroU8::new(2).unwrap(),
            ..RepublisherSettings::default()
        };
        let republisher = Republisher::new(pkarr_client, settings);
        let result = republisher.republish(&public_key).await;

        assert!(matches!(
            result,
            Err(PublishError::InsufficientlyPublished {
                published_nodes_count: 1
            })
        ));
    }
}
