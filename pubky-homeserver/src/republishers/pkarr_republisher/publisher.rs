//!
//! Publishes a single pkarr packet with retries in case it fails.
//!

use pkarr::{mainline::async_dht::AsyncDht, PublicKey, SignedPacket};
use std::{num::NonZeroU8, time::Duration};

use super::verify::count_key_on_dht;

#[derive(thiserror::Error, Debug, Clone)]
pub enum PublishError {
    #[error("Packet has been republished but to an insufficient number of {published_nodes_count} nodes.")]
    InsufficientlyPublished { published_nodes_count: usize },
    #[error(transparent)]
    PublishFailed(#[from] pkarr::errors::PublishError),
}

#[cfg(test)]
impl PublishError {
    fn is_insufficiently_published(&self) -> bool {
        matches!(self, PublishError::InsufficientlyPublished { .. })
    }
}

#[derive(Debug, Clone)]
pub struct PublishInfo {
    /// How many nodes the key got published on.
    pub published_nodes_count: usize,
}

impl PublishInfo {
    pub fn new(published_nodes_count: usize) -> Self {
        Self {
            published_nodes_count,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetrySettings {
    /// Number of max retries to do before aborting.
    pub(crate) max_retries: NonZeroU8,
    /// First retry delay that is then used to calculate the exponential backoff.
    /// Example: 100ms first, then 200ms, 400ms, 800ms and so on.
    pub(crate) initial_retry_delay: Duration,
    /// Cap on the retry delay so the exponential backoff doesn't get out of hand.
    pub(crate) max_retry_delay: Duration,
}

#[cfg(test)]
impl RetrySettings {
    /// Maximum number of republishing retries before giving up.
    pub fn max_retries(&mut self, max_retries: NonZeroU8) -> &mut Self {
        self.max_retries = max_retries;
        self
    }

    /// Maximum duration the republish task exponentionally backs off until it tries again.
    pub fn max_retry_delay(&mut self, duration: Duration) -> &mut Self {
        self.max_retry_delay = duration;
        self
    }

    /// Minimum duration the republish task exponentionally backs off until it tries again.
    pub fn initial_retry_delay(&mut self, duration: Duration) -> &mut Self {
        self.initial_retry_delay = duration;
        self
    }
}

impl Default for RetrySettings {
    fn default() -> Self {
        Self {
            max_retries: NonZeroU8::new(4).expect("should always be > 0"),
            initial_retry_delay: Duration::from_millis(200),
            max_retry_delay: Duration::from_millis(5_000),
        }
    }
}

/// Settings for creating a publisher
#[derive(Debug, Clone)]
pub struct PublisherSettings {
    pub(crate) client: Option<pkarr::Client>,
    pub(crate) min_sufficient_node_publish_count: NonZeroU8,
}

impl Default for PublisherSettings {
    fn default() -> Self {
        Self {
            client: None,
            min_sufficient_node_publish_count: NonZeroU8::new(10).expect("Should always be > 0"),
        }
    }
}

impl PublisherSettings {
    /// Set a custom pkarr client
    pub fn pkarr_client(&mut self, client: pkarr::Client) -> &mut Self {
        self.client = Some(client);
        self
    }

    /// Set the minimum sufficient number of nodes a key needs to be stored in
    /// to be considered a success
    pub fn min_sufficient_node_publish_count(&mut self, count: NonZeroU8) -> &mut Self {
        self.min_sufficient_node_publish_count = count;
        self
    }
}

/// Tries to publish a single key and verifies the keys has been published to
/// a sufficient number of nodes.
#[derive(Debug, Clone)]
pub struct Publisher {
    packet: SignedPacket,
    client: pkarr::Client,
    dht: AsyncDht,
    min_sufficient_node_publish_count: NonZeroU8,
}

impl Publisher {
    pub fn new_with_settings(
        packet: SignedPacket,
        settings: PublisherSettings,
    ) -> Result<Self, pkarr::errors::BuildError> {
        let client = match &settings.client {
            Some(c) => c.clone(),
            None => pkarr::Client::builder().build()?,
        };
        let dht = client.dht().expect("infallible").as_async();
        Ok(Self {
            packet,
            client,
            dht,
            min_sufficient_node_publish_count: settings.min_sufficient_node_publish_count,
        })
    }

    /// Get the public key of the signer of the packet
    fn get_public_key(&self) -> PublicKey {
        self.packet.public_key()
    }

    /// Publish a single public key.
    pub async fn publish_once(&self) -> Result<PublishInfo, PublishError> {
        if let Err(e) = self.client.publish(&self.packet, None).await {
            return Err(e.into());
        }

        // TODO: This counting could really be done with the put response in the mainline library already. It's not exposed though.
        // This would really speed up the publishing and reduce the load on the DHT.
        // -- Sev April 2025 --
        let published_nodes_count = count_key_on_dht(&self.get_public_key(), &self.dht).await;
        if published_nodes_count < self.min_sufficient_node_publish_count.get().into() {
            return Err(PublishError::InsufficientlyPublished {
                published_nodes_count,
            });
        }

        Ok(PublishInfo::new(published_nodes_count))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use pkarr::{dns::Name, Keypair, PublicKey, SignedPacket};

    use super::{PublishError, Publisher, PublisherSettings};

    fn sample_packet() -> (PublicKey, SignedPacket) {
        let key = Keypair::random();
        let packet = pkarr::SignedPacketBuilder::default()
            .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
            .build(&key)
            .unwrap();
        (key.public_key(), packet)
    }

    #[tokio::test]
    async fn single_key_republish_success() {
        let dht = pkarr::mainline::Testnet::builder(3)
            .seeded(false)
            .build()
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let (_, packet) = sample_packet();

        let required_nodes = 3;
        let mut settings = PublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Publisher::new_with_settings(packet, settings).unwrap();
        let res = publisher.publish_once().await;
        assert!(res.is_ok());
        let success = res.unwrap();
        assert_eq!(success.published_nodes_count, 3);
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let dht = pkarr::mainline::Testnet::builder(3)
            .seeded(false)
            .build()
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let (_, packet) = sample_packet();

        let required_nodes = 4;
        let mut settings = PublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Publisher::new_with_settings(packet, settings).unwrap();
        let res = publisher.publish_once().await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.is_insufficiently_published());
        if let PublishError::InsufficientlyPublished {
            published_nodes_count,
        } = err
        {
            assert_eq!(published_nodes_count, 3);
        };
    }
}
