//!
//! Republishes a single public key with retries in case it fails.
//!
use pkarr::PublicKey;
use pkarr::SignedPacket;
use std::{num::NonZeroU8, sync::Arc, time::Duration};

use crate::{
    publisher::{PublishError, Publisher, PublisherSettings},
    RetrySettings,
};

#[derive(thiserror::Error, Debug, Clone)]
pub enum RepublishError {
    #[error("The packet can't be resolved on the DHT and therefore can't be republished.")]
    Missing,
    #[error(transparent)]
    PublishFailed(#[from] PublishError),
}

impl RepublishError {
    pub fn is_missing(&self) -> bool {
        if let RepublishError::Missing = self {
            return true;
        }
        false
    }

    pub fn is_publish_failed(&self) -> bool {
        if let RepublishError::PublishFailed { .. } = self {
            return true;
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct RepublishInfo {
    /// How many nodes the key got published on.
    pub published_nodes_count: usize,
    /// Number of publishing attempts needed to successfully republish.
    pub attempts_needed: usize,
    /// Whether the `republish_condition` was negative.
    pub condition_failed: bool,
}

impl RepublishInfo {
    pub fn new(
        published_nodes_count: usize,
        attempts_needed: usize,
        should_republish_condition_failed: bool,
    ) -> Self {
        Self {
            published_nodes_count,
            attempts_needed,
            condition_failed: should_republish_condition_failed,
        }
    }
}

pub type RepublishCondition = dyn Fn(&SignedPacket) -> bool + Send + Sync;

/// Settings for creating a republisher
#[derive(Clone)]
pub struct RepublisherSettings {
    pub(crate) client: Option<pkarr::Client>,
    pub(crate) min_sufficient_node_publish_count: NonZeroU8,
    pub(crate) retry_settings: RetrySettings,
    pub(crate) republish_condition: Option<Arc<RepublishCondition>>,
}

impl std::fmt::Debug for RepublisherSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepublisherSettings")
            .field("client", &self.client)
            .field(
                "min_sufficient_node_publish_count",
                &self.min_sufficient_node_publish_count,
            )
            .field("retry_settings", &self.retry_settings)
            .finish_non_exhaustive()
    }
}

impl RepublisherSettings {
    pub fn new() -> Self {
        Self::default()
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

    /// Set a closure that determines whether a packet should be republished
    pub fn republish_condition<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&SignedPacket) -> bool + Send + Sync + 'static,
    {
        self.republish_condition = Some(Arc::new(f));
        self
    }
}

impl Default for RepublisherSettings {
    fn default() -> Self {
        Self {
            client: None,
            min_sufficient_node_publish_count: NonZeroU8::new(10).expect("Should always be > 0"),
            retry_settings: RetrySettings::default(),
            republish_condition: None,
        }
    }
}

/// Tries to republish a single key.
/// Retries in case of errors with an exponential backoff.
pub struct Republisher {
    pub public_key: PublicKey,
    client: pkarr::Client,
    min_sufficient_node_publish_count: NonZeroU8,
    retry_settings: RetrySettings,
    republish_condition: Arc<dyn Fn(&SignedPacket) -> bool + Send + Sync>,
}

impl std::fmt::Debug for Republisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Republisher")
            .field("public_key", &self.public_key)
            .field("client", &self.client)
            .field(
                "min_sufficient_node_publish_count",
                &self.min_sufficient_node_publish_count,
            )
            .field("retry_settings", &self.retry_settings)
            .finish_non_exhaustive()
    }
}

impl Republisher {
    /// Creates a new Republisher;
    pub fn new(public_key: PublicKey) -> Result<Self, pkarr::errors::BuildError> {
        let settings = RepublisherSettings::default();
        Self::new_with_settings(public_key, settings)
    }

    pub fn new_with_settings(
        public_key: PublicKey,
        settings: RepublisherSettings,
    ) -> Result<Self, pkarr::errors::BuildError> {
        let client = match &settings.client {
            Some(c) => c.clone(),
            None => pkarr::Client::builder().build()?,
        };
        Ok(Republisher {
            public_key,
            client,
            min_sufficient_node_publish_count: settings.min_sufficient_node_publish_count,
            retry_settings: settings.retry_settings,
            republish_condition: settings
                .republish_condition
                .unwrap_or_else(|| Arc::new(|_| true)),
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
    pub async fn republish_once(&self) -> Result<RepublishInfo, RepublishError> {
        let packet = self.client.resolve_most_recent(&self.public_key).await;
        if packet.is_none() {
            return Err(RepublishError::Missing);
        }
        let packet = packet.unwrap();

        // Check if the packet should be republished
        if !(self.republish_condition)(&packet) {
            return Ok(RepublishInfo::new(0, 1, true));
        }

        let mut settings = PublisherSettings::default();
        settings
            .pkarr_client(self.client.clone())
            .min_sufficient_node_publish_count(self.min_sufficient_node_publish_count);
        let publisher = Publisher::new_with_settings(packet, settings)
            .expect("infallible because pkarr client provided");
        match publisher.publish_once().await {
            Ok(info) => Ok(RepublishInfo::new(info.published_nodes_count, 1, false)),
            Err(e) => Err(e.into()),
        }
    }

    // Republishes the key with an exponential backoff
    pub async fn republish(&self) -> Result<RepublishInfo, RepublishError> {
        let max_retries = self.retry_settings.max_retries.get();
        let mut retry_count = 0;
        let mut last_error: Option<RepublishError> = None;
        while retry_count < max_retries {
            match self.republish_once().await {
                Ok(mut success) => {
                    success.attempts_needed = retry_count as usize + 1;
                    return Ok(success);
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

        Err(last_error.expect("infallible"))
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU8, time::Duration};

    use crate::republisher::{Republisher, RepublisherSettings};
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
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder
            .no_default_network()
            .bootstrap(&dht.bootstrap)
            .no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Republisher::new_with_settings(public_key, settings).unwrap();
        let res = publisher.republish_once().await;
        assert!(res.is_ok());
        let success = res.unwrap();
        assert_eq!(success.published_nodes_count, 1);
    }

    #[tokio::test]
    async fn single_key_republish_missing() {
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        let publisher = Republisher::new_with_settings(public_key, settings).unwrap();
        let res = publisher.republish_once().await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.is_missing());
    }

    #[tokio::test]
    async fn retry_delay() {
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        settings
            .retry_settings
            .max_retries(NonZeroU8::new(10).unwrap())
            .initial_retry_delay(Duration::from_millis(100))
            .max_retry_delay(Duration::from_secs(10));
        let publisher = Republisher::new_with_settings(public_key, settings).unwrap();

        let first_delay = publisher.get_retry_delay(0);
        assert_eq!(first_delay.as_millis(), 100);
        let second_delay = publisher.get_retry_delay(1);
        assert_eq!(second_delay.as_millis(), 200);
        let third_delay = publisher.get_retry_delay(2);
        assert_eq!(third_delay.as_millis(), 400);
        let ninth_delay = publisher.get_retry_delay(9);
        assert_eq!(ninth_delay.as_millis(), 10_000);
    }

    #[tokio::test]
    async fn republish_retry_missing() {
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap());
        settings
            .retry_settings
            .max_retries(NonZeroU8::new(3).unwrap())
            .initial_retry_delay(Duration::from_millis(100));
        let publisher = Republisher::new_with_settings(public_key, settings).unwrap();
        let res = publisher.republish().await;

        assert!(res.is_err());
        assert!(res.unwrap_err().is_missing());
    }

    #[tokio::test]
    async fn republish_with_condition_fail() {
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client.clone())
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
            // Only republish if the packet has a TTL greater than 300
            .republish_condition(|_| false);

        let publisher = Republisher::new_with_settings(public_key.clone(), settings).unwrap();
        let res = publisher.republish_once().await;
        assert!(res.is_ok());
        let info = res.unwrap();
        assert_eq!(info.published_nodes_count, 0);
        assert!(info.condition_failed);
    }

    #[tokio::test]
    async fn republish_with_condition_success() {
        let dht = tokio::task::spawn_blocking(|| pkarr::mainline::Testnet::new(1))
            .await
            .expect("spawn_blocking panicked")
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client.clone())
            .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
            // Only republish if the packet has a TTL greater than 300
            .republish_condition(|_| true);

        let publisher = Republisher::new_with_settings(public_key.clone(), settings).unwrap();
        let res = publisher.republish_once().await;
        assert!(res.is_ok());
        let info = res.unwrap();
        assert_eq!(info.published_nodes_count, 1);
        assert!(!info.condition_failed);
    }
}
