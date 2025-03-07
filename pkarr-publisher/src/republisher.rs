//!
//! Republishes a single public key with retries in case it fails.
//!
use pkarr::PublicKey;
use std::{num::NonZeroU8, time::Duration};

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
        return false;
    }

    pub fn is_publish_failed(&self) -> bool {
        if let RepublishError::PublishFailed { .. } = self {
            return true;
        }
        return false;
    }
}

#[derive(Debug, Clone)]
pub struct RepublishInfo {
    /// How many nodes the key got published on.
    pub published_nodes_count: usize,
    /// Number of publishing attempts needed to successfully republish.
    pub attempts_needed: usize,
}

impl RepublishInfo {
    pub fn new(published_nodes_count: usize, attempts_needed: usize) -> Self {
        Self {
            published_nodes_count,
            attempts_needed,
        }
    }
}

/// Settings for creating a republisher
#[derive(Debug, Clone)]
pub struct RepublisherSettings {
    pub(crate) client: Option<pkarr::Client>,
    pub(crate) min_sufficient_node_publish_count: NonZeroU8,
    pub(crate) retry_settings: RetrySettings,
}

impl RepublisherSettings {
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
}

/// Tries to republish a single key.
/// Retries in case of errors with an exponential backoff.
#[derive(Debug, Clone)]
pub struct Republisher {
    pub public_key: PublicKey,
    client: pkarr::Client,
    min_sufficient_node_publish_count: NonZeroU8,
    retry_settings: RetrySettings,
}

impl Republisher {
    /// Creates a new Republisher;
    pub fn new(public_key: PublicKey) -> Result<Self, pkarr::errors::BuildError> {
        let settings = RepublisherSettings::new();
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
            public_key: public_key,
            client,
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
    pub async fn republish_once(&self) -> Result<RepublishInfo, RepublishError> {
        let packet = self.client.resolve_most_recent(&self.public_key).await;
        if packet.is_none() {
            return Err(RepublishError::Missing);
        }
        let packet = packet.unwrap();

        let mut settings = PublisherSettings::new();
        settings
            .pkarr_client(self.client.clone())
            .min_sufficient_node_publish_count(self.min_sufficient_node_publish_count);
        let publisher = Publisher::new_with_settings(self.public_key.clone(), packet, settings)
            .expect("infalliable because pkarr client provided");
        match publisher.publish_once().await {
            Ok(info) => return Ok(RepublishInfo::new(info.published_nodes_count, 1)),
            Err(e) => {
                let publish_error: PublishError = e.into();
                return Err(publish_error.into());
            }
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
            tracing::info!(
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

    use crate::republisher::{Republisher, RepublisherSettings};
    use pkarr::{dns::Name, Keypair, PublicKey};
    use pubky_testnet::Testnet;

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
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_key = publish_sample_packets(&pkarr_client).await;

        let required_nodes = 1;
        let mut settings = RepublisherSettings::new();
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
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::new();
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
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::new();
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
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let mut settings = RepublisherSettings::new();
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
}
