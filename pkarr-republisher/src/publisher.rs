//!
//! Publishes a single pkarr packet with retries in case it fails.
//!

use pkarr::{mainline::async_dht::AsyncDht, PublicKey, SignedPacket};
use std::{num::NonZeroU8, time::Duration};

use crate::verify::count_key_on_dht;

#[derive(thiserror::Error, Debug, Clone)]
pub enum PublishError {
    #[error("Packet has been republished but to an insufficient number of {published_nodes_count} nodes.")]
    InsuffientlyPublished { published_nodes_count: usize },
    #[error(transparent)]
    PublishFailed(#[from] pkarr::errors::PublishError),
}

impl PublishError {
    pub fn is_insufficiently_published(&self) -> bool {
        if let PublishError::InsuffientlyPublished { .. } = self {
            return true;
        }
        return false;
    }

    pub fn is_publish_failed(&self) -> bool {
        if let PublishError::PublishFailed { .. } = self {
            return true;
        }
        return false;
    }
}

#[derive(Debug, Clone)]
pub struct PublishInfo {
    /// How many nodes the key got published on.
    pub published_nodes_count: usize,
    /// Number of publishing attempts needed to successfully publish.
    pub attempts_needed: usize,
}

impl PublishInfo {
    pub fn new(published_nodes_count: usize, attempts_needed: usize) -> Self {
        Self {
            published_nodes_count,
            attempts_needed,
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

impl RetrySettings {
    pub fn new() -> Self {
        Self {
            max_retries: NonZeroU8::new(4).unwrap(),
            initial_retry_delay: Duration::from_millis(200),
            max_retry_delay: Duration::from_millis(5_000),
        }
    }
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

/// Settings for creating a republisher
#[derive(Debug, Clone)]
pub struct PublisherSettings {
    pub(crate) client: Option<pkarr::Client>,
    pub(crate) min_sufficient_node_publish_count: NonZeroU8,
    pub retry_settings: RetrySettings,
}

impl PublisherSettings {
    // Create new builder
    pub fn new() -> Self {
        Self {
            client: None,
            min_sufficient_node_publish_count: NonZeroU8::new(10).unwrap(),
            retry_settings: RetrySettings::new(),
        }
    }

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

    /// Set settings in relation to retries.
    pub fn retry_settings(&mut self, settings: RetrySettings) -> &mut Self {
        self.retry_settings = settings;
        self
    }
}

/// Tries to publish a single key and verifies the keys has been published to
/// a sufficient number of nodes.
/// Retries in case of errors with an exponential backoff.
#[derive(Debug, Clone)]
pub struct Publisher {
    pub public_key: PublicKey,
    pub packet: SignedPacket,
    client: pkarr::Client,
    dht: AsyncDht,
    min_sufficient_node_publish_count: NonZeroU8,
    retry_settings: RetrySettings,
}

impl Publisher {
    /// Creates a new Publisher with a new pkarr client.
    pub fn new(
        public_key: PublicKey,
        packet: SignedPacket,
    ) -> Result<Self, pkarr::errors::BuildError> {
        let settings = PublisherSettings::new();
        Self::new_with_settings(public_key, packet, settings)
    }

    pub fn new_with_settings(
        public_key: PublicKey,
        packet: SignedPacket,
        settings: PublisherSettings,
    ) -> Result<Self, pkarr::errors::BuildError> {
        let client = match &settings.client {
            Some(c) => c.clone(),
            None => pkarr::Client::builder().build()?,
        };
        let dht = client.dht().expect("infalliable").as_async();
        Ok(Self {
            public_key: public_key,
            packet,
            client,
            dht,
            min_sufficient_node_publish_count: settings.min_sufficient_node_publish_count,
            retry_settings: settings.retry_settings,
        })
    }

    /// Exponential backoff delay starting with `INITIAL_DELAY_MS` and maxing out at  `MAX_DELAY_MS`
    fn get_retry_delay(&self, retry_count: u8) -> Duration {
        let initial_ms = self.retry_settings.initial_retry_delay.as_millis() as u64;
        let multiplicator = 2u64.pow(retry_count as u32);
        let delay_ms = initial_ms * multiplicator;
        let delay = Duration::from_millis(delay_ms);
        delay.min(self.retry_settings.max_retry_delay)
    }

    /// Republish a single public key.
    pub async fn publish_once(&self) -> Result<PublishInfo, PublishError> {
        if let Err(e) = self.client.publish(&self.packet, None).await {
            return Err(e.into());
        }

        // TODO: This counting could really be done with the put response in the mainline library already. It's not exposed though.
        // This would really speed up the publishing and reduce the load on the DHT.
        // -- Sev April 2025 --
        let published_nodes_count = count_key_on_dht(&self.public_key, &self.dht).await;
        if published_nodes_count < self.min_sufficient_node_publish_count.get().into() {
            return Err(PublishError::InsuffientlyPublished {
                published_nodes_count,
            });
        }

        Ok(PublishInfo::new(published_nodes_count, 1))
    }

    // Publishes the key with an exponential backoff
    pub async fn publish(&self) -> Result<PublishInfo, PublishError> {
        let max_retries = self.retry_settings.max_retries.get();
        let mut retry_count = 0;
        let mut last_error: Option<PublishError> = None;
        while retry_count < max_retries {
            match self.publish_once().await {
                Ok(mut info) => {
                    info.attempts_needed = retry_count as usize + 1;
                    return Ok(info);
                }
                Err(e) => {
                    tracing::debug!(
                        "{retry_count}/{max_retries} Failed to publish {}: {e}",
                        self.public_key
                    );
                    last_error = Some(e);
                }
            }

            let delay = self.get_retry_delay(retry_count);
            retry_count += 1;
            tracing::debug!(
                "{} {retry_count}/{max_retries} Sleep for {delay:?} before trying again.",
                self.public_key
            );
            tokio::time::sleep(delay).await;
        }

        return Err(last_error.expect("infalliable"));
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU8, time::Duration};

    use pkarr::{dns::Name, Keypair, PublicKey, SignedPacket};
    use pubky_testnet::Testnet;

    use crate::publisher::{PublishError, Publisher, PublisherSettings};

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
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let (key, packet) = sample_packet();

        let required_nodes = 1;
        let mut settings = PublisherSettings::new();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Publisher::new_with_settings(key, packet, settings).unwrap();
        let res = publisher.publish_once().await;
        assert!(res.is_ok());
        let success = res.unwrap();
        assert_eq!(success.published_nodes_count, 1);
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let (key, packet) = sample_packet();

        let required_nodes = 2;
        let mut settings = PublisherSettings::new();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Publisher::new_with_settings(key, packet, settings).unwrap();
        let res = publisher.publish_once().await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.is_insufficiently_published());
        if let PublishError::InsuffientlyPublished {
            published_nodes_count,
        } = err
        {
            assert_eq!(published_nodes_count, 1);
        };
    }

    #[tokio::test]
    async fn retry_delay() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let (key, packet) = sample_packet();

        let required_nodes = 1;
        let mut settings = PublisherSettings::new();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        settings
            .retry_settings
            .max_retries(NonZeroU8::new(10).unwrap())
            .initial_retry_delay(Duration::from_millis(100))
            .max_retry_delay(Duration::from_secs(10));
        let publisher = Publisher::new_with_settings(key, packet, settings).unwrap();

        let first_delay = publisher.get_retry_delay(0);
        assert_eq!(first_delay.as_millis(), 100);
        let second_delay = publisher.get_retry_delay(1);
        assert_eq!(second_delay.as_millis(), 200);
        let third_delay = publisher.get_retry_delay(2);
        assert_eq!(third_delay.as_millis(), 400);
        let ninth_delay = publisher.get_retry_delay(9);
        assert_eq!(ninth_delay.as_millis(), 10_000);
    }
}
