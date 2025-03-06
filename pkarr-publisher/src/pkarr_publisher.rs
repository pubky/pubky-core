use std::{collections::HashMap};
use pkarr::{Client, PublicKey};
use tokio::time::Instant;

use crate::single_key_publisher::{RepublishError, RepublishInfo, RepublisherSettings, SingleKeyRepublisher};

#[derive(Debug, Clone)]
pub struct PkarrRepublisher {
    settings: RepublisherSettings
}


impl PkarrRepublisher {
    pub fn new() -> Result<Self, pkarr::errors::BuildError> {
        let settings = RepublisherSettings::new();
        Ok(Self {
            settings,
        })
    }

    pub fn new_with_settings(mut settings: RepublisherSettings) -> Result<Self, pkarr::errors::BuildError> {
        settings.client = None; // Remove client if it's there because every thread will have it's own.
        Ok(Self {
            settings,
        })
    }

    /// Go through the list of all public keys and republish them serially.
    pub async fn run(&self, public_keys: Vec<PublicKey>) -> HashMap<PublicKey, Result<RepublishInfo, RepublishError>>{
        let mut results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>> = HashMap::with_capacity(public_keys.len());
        tracing::info!("Start to republish {} public keys.", public_keys.len());
        let client = Client::builder().no_relays().build().unwrap();
        let mut local_settings = self.settings.clone();
        local_settings.client = Some(client);
        for key in public_keys {
            let start = Instant::now();

            let republisher = SingleKeyRepublisher::new_with_settings(key.clone(), local_settings.clone()).expect("infalliable");
            let res = republisher.republish().await;

            let elapsed = start.elapsed().as_millis();
            match &res {
                Ok(info) => {
                    tracing::info!("Republished {key} successfully on {} nodes within {elapsed}ms.", info.published_nodes_count)
                },
                Err(e) => {
                    tracing::warn!("Failed to republish public_key {} within {elapsed}ms. {}", key, e);
                }
            }

            results.insert(key.clone(), res);
        }
        results
    }

    pub async fn run_parallel(&self, public_keys: Vec<PublicKey>, thread_count: u8) -> HashMap<PublicKey, Result<RepublishInfo, RepublishError>> {
        let chunk_size = public_keys.len().div_ceil(thread_count as usize);
        let chunks = public_keys.chunks(chunk_size).map(|chunk| chunk.to_vec()).collect::<Vec<_>>();

        // Run in parallel
        let mut handles = vec![];
        for chunk in chunks {
            let publisher = self.clone();
            let handle = tokio::spawn(async move {
                publisher.run(chunk).await
            });
            handles.push(handle);
        };

        // Join results of all tasks
        let mut results = HashMap::with_capacity(public_keys.len());
        for handle in handles {
            let result = handle.await.expect("should have result");
            for entry in result {
                results.insert(entry.0, entry.1);
            }
        }

        results
    }
}



#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use pkarr::{dns::Name, Keypair, PublicKey};
    use pubky_testnet::Testnet;

    use crate::{pkarr_publisher::PkarrRepublisher, single_key_publisher::RepublisherSettings};

    async fn publish_sample_packets(client: &pkarr::Client, count: usize) -> Vec<PublicKey> {
        let keys: Vec<Keypair> = (0..count).map(|_| Keypair::random()).collect();
        for key in keys.iter() {
            let packet = pkarr::SignedPacketBuilder::default().cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600).build(key).unwrap();
            let _ = client.publish(&packet, None).await;
        };

        keys.into_iter().map(|key| key.public_key()).collect()
    }

    #[tokio::test]
    async fn single_key_republish_success() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let settings = RepublisherSettings::new()
        .pkarr_client(pkarr_client)
        .min_sufficient_node_publish_count(NonZeroU8::new(1).unwrap());
        let publisher = PkarrRepublisher::new_with_settings(settings).unwrap();
        let results = publisher.run(public_keys).await;
        let result = results.get(&public_key).unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn single_key_republish_insufficient() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let settings = RepublisherSettings::new()
        .pkarr_client(pkarr_client)
        .min_sufficient_node_publish_count(NonZeroU8::new(2).unwrap());
        let publisher = PkarrRepublisher::new_with_settings(settings).unwrap();
        let results = publisher.run(public_keys).await;
        let result = results.get(&public_key).unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mainnet_missing() {
        let keys: Vec<PublicKey> = (0..100).map(|_| Keypair::random()).map(|key| key.public_key()).collect();

        let publisher = PkarrRepublisher::new().unwrap();
        let results = publisher.run_parallel(keys, 7).await;
    }
}
