use futures_lite::StreamExt;
use pkarr::{mainline::async_dht::AsyncDht, PublicKey};
use std::{num::NonZeroU8, time::Duration};

#[derive(thiserror::Error, Debug, Clone)]
pub enum RepublishError {
    #[error("The packet can't be resolved on the DHT and therefore can't be republished.")]
    Missing,
    #[error("Packet has been republished but to an insufficient number of nodes.")]
    InsuffientlyPublished { published_nodes_count: usize },
    #[error(transparent)]
    PublishFailed(#[from] pkarr::errors::PublishError),
}

impl RepublishError {
    pub fn is_missing(&self) -> bool {
        if let RepublishError::Missing = self {
            return true
        }
        return false
    }

    pub fn is_insufficiently_published(&self) -> bool {
        if let RepublishError::InsuffientlyPublished { .. } = self {
            return true
        }
        return false
    }

    pub fn is_publish_failed(&self) -> bool {
        if let RepublishError::PublishFailed { .. } = self {
            return true
        }
        return false
    }
}

#[derive(Debug, Clone)]
pub struct RepublishInfo {
    /// How many nodes the key got published on.
    pub published_nodes_count: usize,
}

impl RepublishInfo {
    pub fn new(published_nodes_count: usize) -> Self {
        Self {
            published_nodes_count,
        }
    }
}

const DEFAULT_MAX_REPUBLISH_RETRIES: u8 = 4;
const DEFAULT_INITIAL_DELAY_MS: u64 = 1_000;
const DEFAULT_MAX_DELAY_MS: u64 = 10_000;
const DEFAULT_MIN_SUFFIENT_NODE_PUBLISH_COUNT: u8 = 10;

#[derive(Debug, Clone)]
pub struct SingleKeyRepublisherBuilder {
    public_key: PublicKey,
    client: Option<pkarr::Client>,
    min_sufficient_node_publish_count: NonZeroU8,
    max_retries: NonZeroU8,
    initial_retry_delay: Duration,
    max_retry_delay: Duration
}

impl SingleKeyRepublisherBuilder {
    // Create new builder
    pub fn new(public_key: PublicKey) -> Self {
        Self {
            public_key,
            client: None,
            min_sufficient_node_publish_count: NonZeroU8::new(DEFAULT_MIN_SUFFIENT_NODE_PUBLISH_COUNT).unwrap(),
            max_retries: NonZeroU8::new(DEFAULT_MAX_REPUBLISH_RETRIES).unwrap(),
            initial_retry_delay: Duration::from_millis(DEFAULT_INITIAL_DELAY_MS),
            max_retry_delay: Duration::from_millis(DEFAULT_MAX_DELAY_MS)
        }
    }

    /// Set a custom pkarr client
    pub fn pkarr_client(mut self, client: pkarr::Client) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the minimum sufficient number of nodes a key needs to be stored in
    /// to be considered a success
    pub fn min_sufficient_node_publish_count(mut self, count: NonZeroU8) -> Self {
        self.min_sufficient_node_publish_count = count;
        self
    }

    /// Maximum number of republishing retries before giving up.
    pub fn max_retries(mut self, max_retries: NonZeroU8) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Maximum duration the republish task exponentionally backs off until it tries again.
    pub fn max_retry_delay(mut self, duration: Duration) -> Self {
        self.max_retry_delay = duration;
        self
    }

    /// Minimum duration the republish task exponentionally backs off until it tries again.
    pub fn initial_retry_delay(mut self, duration: Duration) -> Self {
        self.initial_retry_delay = duration;
        self
    }

    /// Build republisher
    pub fn build(self) -> Result<SingleKeyRepublisher, pkarr::errors::BuildError> {
        let client = self.client.unwrap_or(
            pkarr::Client::builder().build()?
        );
        let dht = client.dht().expect("infalliable").as_async();
        Ok(SingleKeyRepublisher {
            public_key: self.public_key,
            client,
            dht,
            min_sufficient_node_publish_count: self.min_sufficient_node_publish_count,
            max_retries: self.max_retries,
            initial_retry_delay: self.initial_retry_delay,
            max_retry_delay: self.max_retry_delay
        })
    }
}



/// Tries to republish a single key.
/// Retries in case of errors with an exponential backoff.
#[derive(Debug, Clone)]
pub struct SingleKeyRepublisher {
    pub public_key: PublicKey,
    client: pkarr::Client,
    dht: AsyncDht,
    min_sufficient_node_publish_count: NonZeroU8,
    max_retries: NonZeroU8,
    initial_retry_delay: Duration,
    max_retry_delay: Duration,
}

impl SingleKeyRepublisher {
    /// Creates a new Republisher with a new pkarr client.
    pub fn new(public_key: PublicKey) -> Result<Self, pkarr::errors::BuildError> {
        let client = pkarr::Client::builder().build()?;
        let dht = client.dht().expect("infalliable").as_async();

        Ok(Self {
            public_key,
            client,
            dht,
            min_sufficient_node_publish_count: NonZeroU8::new(DEFAULT_MIN_SUFFIENT_NODE_PUBLISH_COUNT).unwrap(),
            max_retries: NonZeroU8::new(DEFAULT_MAX_REPUBLISH_RETRIES).unwrap(),
            initial_retry_delay: Duration::from_millis(DEFAULT_INITIAL_DELAY_MS),
            max_retry_delay: Duration::from_millis(DEFAULT_MAX_DELAY_MS)
        })
    }

    /// Exponential backoff delay starting with `INITIAL_DELAY_MS` and maxing out at  `MAX_DELAY_MS`
    fn get_retry_delay(&self, retry_count: u8) -> Duration {
        let initial_ms = self.initial_retry_delay.as_millis() as u64;
        let multiplicator = 2u64.pow(retry_count as u32);
        let delay_ms = initial_ms * multiplicator;
        let delay = Duration::from_millis(delay_ms);
        delay.min(self.max_retry_delay)
    }

    /// Count the number of nodes that the public key is published on.
    async fn count_published_nodes(&self, public_key: &PublicKey) -> usize {
        let mut response_count = 0;
        let mut stream = self.dht.get_mutable(public_key.as_bytes(), None, None);
        while let Some(_) = stream.next().await {
            response_count += 1;
        }
        response_count
    }

    /// Republish a single public key.
    pub async fn republish_once(&self) -> Result<RepublishInfo, RepublishError> {
        let packet = self.client.resolve_most_recent(&self.public_key).await;
        if packet.is_none() {
            return Err(RepublishError::Missing);
        }
        let packet = packet.unwrap();
        if let Err(e) = self.client.publish(&packet, None).await {
            return Err(e.into());
        }
        let published_nodes_count = self.count_published_nodes(&self.public_key).await;
        if published_nodes_count < self.min_sufficient_node_publish_count.get().into() {
            return Err(RepublishError::InsuffientlyPublished {
                published_nodes_count,
            });
        }

        Ok(RepublishInfo::new(published_nodes_count))
    }

    // Republishes the key with an exponential backoff
    pub async fn republish(
        &self,
    ) -> Result<RepublishInfo, RepublishError> {
        let max_retries = self.max_retries.get();
        let mut retry_count = 0;
        let mut last_error: Option<RepublishError> = None;
        while retry_count < max_retries {
            match self.republish_once().await {
                Ok(success) => return Ok(success),
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

    use crate::single_key_publisher::{RepublishError, SingleKeyRepublisherBuilder};
    use pkarr::{dns::Name, Keypair, PublicKey};
    use pubky_testnet::Testnet;

    async fn publish_sample_packets(client: &pkarr::Client, count: usize) -> Vec<PublicKey> {
        let keys: Vec<Keypair> = (0..count).map(|_| Keypair::random()).collect();
        for key in keys.iter() {
            let packet = pkarr::SignedPacketBuilder::default()
                .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
                .build(key)
                .unwrap();
            let _ = client.publish(&packet, None).await;
        }

        keys.into_iter().map(|key| key.public_key()).collect()
    }

    #[tokio::test]
    async fn single_key_republish_success() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let publisher = SingleKeyRepublisherBuilder::new(public_key).pkarr_client(pkarr_client).min_sufficient_node_publish_count(NonZeroU8::new(1).unwrap()).build().unwrap();
        let res = publisher.republish_once().await;
        assert!(res.is_ok());
        let success = res.unwrap();
        assert_eq!(success.published_nodes_count, 1);
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let required_nodes = 2;
        let publisher = SingleKeyRepublisherBuilder::new(public_key).pkarr_client(pkarr_client).min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap()).build().unwrap();
        let res = publisher.republish_once().await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.is_insufficiently_published());
        if let RepublishError::InsuffientlyPublished { published_nodes_count } = err {
            assert_eq!(published_nodes_count, 1);
        };
    }

    #[tokio::test]
    async fn single_key_republish_missing() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_key = Keypair::random().public_key();

        let required_nodes = 1;
        let publisher = SingleKeyRepublisherBuilder::new(public_key).pkarr_client(pkarr_client).min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap()).build().unwrap();
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
        let publisher = SingleKeyRepublisherBuilder::new(public_key)
        .pkarr_client(pkarr_client)
        .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
        .max_retries(NonZeroU8::new(10).unwrap())
        .initial_retry_delay(Duration::from_millis(100))
        .max_retry_delay(Duration::from_secs(10))
        .build().unwrap();
        
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
        let publisher = SingleKeyRepublisherBuilder::new(public_key)
        .pkarr_client(pkarr_client)
        .min_sufficient_node_publish_count(NonZeroU8::new(required_nodes).unwrap())
        .max_retries(NonZeroU8::new(3).unwrap())
        .initial_retry_delay(Duration::from_millis(100))
        .build().unwrap();
        let res = publisher.republish().await;

        assert!(res.is_err());
        assert!(res.unwrap_err().is_missing());
    }

}
