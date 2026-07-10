use super::republish_summary::{RepublishResult, RepublishSummary};
use super::republisher::RepublisherSettings;
use super::resilient_client::{ResilientClient, ResilientClientBuilderError};
use futures_util::{stream::FuturesUnordered, TryStreamExt};
use pkarr::{ClientBuilder, PublicKey};
use std::{num::NonZeroUsize, sync::Mutex};
use tokio::time::Instant;

type PublicKeyQueue = Mutex<Vec<PublicKey>>;

/// Republish multiple keys serially or concurrently.
#[derive(Debug, Clone, Default)]
pub struct MultiRepublisher {
    settings: RepublisherSettings,
    client_builder: ClientBuilder,
}

impl MultiRepublisher {
    /// Create a new republisher with the settings.
    pub fn new_with_settings(settings: RepublisherSettings, client_builder: ClientBuilder) -> Self {
        Self {
            settings,
            client_builder,
        }
    }

    /// Republish keys concurrently with at most `max_concurrent_workers` active clients.
    ///
    /// # Errors
    ///
    /// Returns an error if a worker cannot build a DHT-enabled pkarr client.
    pub async fn run(
        &self,
        public_keys: Vec<PublicKey>,
        max_concurrent_workers: NonZeroUsize,
    ) -> Result<RepublishSummary, ResilientClientBuilderError> {
        let worker_count = max_concurrent_workers.get().min(public_keys.len());
        let public_keys = Mutex::new(public_keys);

        (0..worker_count)
            .map(|_| self.run_worker(&public_keys))
            .collect::<FuturesUnordered<_>>()
            .try_fold(RepublishSummary::default(), merge_summaries)
            .await
    }

    async fn run_worker(
        &self,
        public_keys: &PublicKeyQueue,
    ) -> Result<RepublishSummary, ResilientClientBuilderError> {
        let client = self.client_builder.build()?;
        let client = ResilientClient::new(client, self.settings.clone())?;

        let mut summary = RepublishSummary::default();
        while let Some(public_key) = pop(public_keys) {
            summary.record(republish_key(&client, &public_key).await);
        }
        Ok(summary)
    }
}

async fn republish_key(client: &ResilientClient, public_key: &PublicKey) -> RepublishResult {
    let start = Instant::now();
    let result = client.republish(public_key).await;
    let elapsed = start.elapsed().as_millis();

    match &result {
        Ok(info) => tracing::info!(
            "Republished {public_key} successfully on {} nodes within {elapsed}ms. attempts={}",
            info.published_nodes_count,
            info.attempts_needed
        ),
        Err(error) => tracing::warn!(
            "Failed to republish public_key {public_key} within {elapsed}ms. {error}"
        ),
    }
    result
}

async fn merge_summaries(
    summary: RepublishSummary,
    worker_summary: RepublishSummary,
) -> Result<RepublishSummary, ResilientClientBuilderError> {
    Ok(summary.merge(worker_summary))
}

fn pop(public_keys: &PublicKeyQueue) -> Option<PublicKey> {
    public_keys
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .pop()
}

#[cfg(test)]
mod tests {
    use std::num::{NonZeroU8, NonZeroUsize};

    use pkarr::{dns::Name, Keypair, PublicKey};

    use super::{MultiRepublisher, RepublishSummary};
    use crate::republishers::pkarr_republisher::republisher::RepublisherSettings;

    async fn publish_sample_packets(client: &pkarr::Client, count: usize) -> Vec<PublicKey> {
        let keys: Vec<Keypair> = (0..count).map(|_| Keypair::random()).collect();
        for key in &keys {
            let packet = pkarr::SignedPacketBuilder::default()
                .cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600)
                .build(key)
                .unwrap();
            client
                .publish(&packet, None)
                .await
                .expect("sample packet should publish");
        }

        keys.into_iter().map(|key| key.public_key()).collect()
    }

    async fn republish_single_key(
        min_sufficient_node_publish_count: NonZeroU8,
    ) -> RepublishSummary {
        let dht = pkarr::mainline::Testnet::builder(3)
            .seeded(false)
            .build()
            .unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let mut settings = RepublisherSettings::default();
        settings.min_sufficient_node_publish_count(min_sufficient_node_publish_count);

        MultiRepublisher::new_with_settings(settings, pkarr_builder)
            .run(public_keys, NonZeroUsize::MIN)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn single_key_republish_success() {
        let summary = republish_single_key(NonZeroU8::new(3).unwrap()).await;

        assert_eq!(summary.len(), 1);
        assert_eq!(summary.success_count(), 1);
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let summary = republish_single_key(NonZeroU8::new(4).unwrap()).await;

        assert_eq!(summary.len(), 1);
        assert_eq!(summary.publishing_failed_count(), 1);
    }
}
