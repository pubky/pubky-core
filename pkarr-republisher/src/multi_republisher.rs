use crate::{
    republisher::{RepublishError, RepublishInfo, RepublisherSettings},
    ResilientClient, ResilientClientBuilderError,
};
use pkarr::PublicKey;
use std::collections::HashMap;
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub struct MultiRepublishResult {
    results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>>,
}

impl MultiRepublishResult {
    pub fn new(results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>>) -> Self {
        Self { results }
    }

    /// Number of keys
    pub fn len(&self) -> usize {
        self.results.len()
    }

    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// All keys
    pub fn all_keys(&self) -> Vec<PublicKey> {
        self.results.keys().cloned().collect()
    }

    /// Successfully published keys
    pub fn success(&self) -> Vec<PublicKey> {
        self.results
            .iter()
            .filter(|(_, result)| result.is_ok())
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Keys that failed to publish
    pub fn publishing_failed(&self) -> Vec<PublicKey> {
        self.results
            .iter()
            .filter(|(_, val)| {
                if let Err(e) = val {
                    return e.is_publish_failed();
                }
                false
            })
            .map(|entry| entry.0.clone())
            .collect()
    }

    /// Keys that are missing and could not be republished
    pub fn missing(&self) -> Vec<PublicKey> {
        self.results
            .iter()
            .filter(|(_, val)| {
                if let Err(e) = val {
                    return e.is_missing();
                }
                false
            })
            .map(|entry| entry.0.clone())
            .collect()
    }
}

/// Republish multiple keys in a serially or multi-threaded way/
#[derive(Debug, Clone, Default)]
pub struct MultiRepublisher {
    settings: RepublisherSettings,
    client_builder: pkarr::ClientBuilder,
}

impl MultiRepublisher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new republisher with the settings.
    /// The republisher ignores the settings.client but instead uses the client_builder to create multiple
    /// pkarr clients instead of just one.
    pub fn new_with_settings(
        mut settings: RepublisherSettings,
        client_builder: Option<pkarr::ClientBuilder>,
    ) -> Self {
        settings.client = None; // Remove client if it's there because every thread will have it's own.
        let builder = client_builder.unwrap_or_default();
        Self {
            settings,
            client_builder: builder,
        }
    }

    /// Go through the list of all public keys and republish them serially.
    async fn run_serially(
        &self,
        public_keys: Vec<PublicKey>,
    ) -> Result<
        HashMap<PublicKey, Result<RepublishInfo, RepublishError>>,
        ResilientClientBuilderError,
    > {
        let mut results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>> =
            HashMap::with_capacity(public_keys.len());
        tracing::debug!("Start to republish {} public keys.", public_keys.len());
        // TODO: Inspect pkarr reliability.
        // pkarr client gets really unreliable when used in parallel. To get around this, we use one client per run().
        let client = self.client_builder.clone().build()?;
        let rclient =
            ResilientClient::new_with_client(client, self.settings.retry_settings.clone())?;
        for key in public_keys {
            let start = Instant::now();
            let res = rclient
                .republish(
                    key.clone(),
                    Some(self.settings.min_sufficient_node_publish_count),
                )
                .await;

            let elapsed = start.elapsed().as_millis();
            match &res {
                Ok(info) => {
                    tracing::info!(
                        "Republished {key} successfully on {} nodes within {elapsed}ms. attemps={}",
                        info.published_nodes_count,
                        info.attempts_needed
                    )
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to republish public_key {} within {elapsed}ms. {}",
                        key,
                        e
                    );
                }
            }

            results.insert(key.clone(), res);
        }
        Ok(results)
    }

    /// Republish keys in a parallel fashion, using multiple threads for better performance.
    /// A good thread size is around 10 for most computers. With high performance cores, you can push
    /// it to 40+.
    pub async fn run(
        &self,
        public_keys: Vec<PublicKey>,
        thread_count: u8,
    ) -> Result<MultiRepublishResult, ResilientClientBuilderError> {
        let chunk_size = public_keys.len().div_ceil(thread_count as usize);
        let chunks = public_keys
            .chunks(chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect::<Vec<_>>();

        // Run in parallel
        let mut handles = vec![];
        for chunk in chunks {
            let publisher = self.clone();
            let handle = tokio::spawn(async move { publisher.run_serially(chunk).await });
            handles.push(handle);
        }

        // Join results of all tasks
        let mut results = HashMap::with_capacity(public_keys.len());
        for handle in handles {
            let join_result = handle.await;
            if let Err(e) = join_result {
                tracing::error!("Failed to join handle in MultiRepublisher::run: {e}");
                continue;
            }
            let result = join_result.unwrap()?;
            for entry in result {
                results.insert(entry.0, entry.1);
            }
        }

        Ok(MultiRepublishResult::new(results))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use pkarr::{dns::Name, Keypair, PublicKey};

    use crate::{multi_republisher::MultiRepublisher, republisher::RepublisherSettings};

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
        let dht = pkarr::mainline::Testnet::new(3).unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();

        let public_keys = publish_sample_packets(&pkarr_client, 1).await;
        let public_key = public_keys.first().unwrap().clone();

        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(4).unwrap());
        let publisher = MultiRepublisher::new_with_settings(settings, Some(pkarr_builder));
        let results = publisher.run_serially(public_keys).await.unwrap();
        let result = results.get(&public_key).unwrap();
        if let Err(e) = result {
            println!("Err {e}");
        }
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let dht = pkarr::mainline::Testnet::new(3).unwrap();
        let mut pkarr_builder = pkarr::ClientBuilder::default();
        pkarr_builder.bootstrap(&dht.bootstrap).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();

        let public_keys = publish_sample_packets(&pkarr_client, 1).await;
        let public_key = public_keys.first().unwrap().clone();

        let mut settings = RepublisherSettings::default();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(5).unwrap());
        let publisher = MultiRepublisher::new_with_settings(settings, Some(pkarr_builder));
        let results = publisher.run_serially(public_keys).await.unwrap();
        let result = results.get(&public_key).unwrap();
        assert!(result.is_err());
    }
}
