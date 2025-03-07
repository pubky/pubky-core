use crate::republisher::{RepublishError, RepublishInfo, Republisher, RepublisherSettings};
use pkarr::{errors::BuildError, ClientBuilder, PublicKey};
use std::collections::HashMap;
use tokio::time::Instant;

#[derive(Debug, Clone)]
pub struct MultiRepublisher {
    settings: RepublisherSettings,
    client_builder: pkarr::ClientBuilder,
}

impl MultiRepublisher {
    pub fn new() -> Result<Self, pkarr::errors::BuildError> {
        let settings = RepublisherSettings::new();
        Ok(Self {
            settings,
            client_builder: pkarr::ClientBuilder::default(),
        })
    }

    /// Create a new republisher with the settings.
    /// The republisher ignores the settings.client but instead uses the client_builder to create multiple
    /// pkarr clients instead of just one.
    pub fn new_with_settings(
        mut settings: RepublisherSettings,
        client_builder: Option<pkarr::ClientBuilder>,
    ) -> Self {
        settings.client = None; // Remove client if it's there because every thread will have it's own.
        let builder = client_builder.unwrap_or(ClientBuilder::default());
        Self {
            settings,
            client_builder: builder,
        }
    }

    /// Go through the list of all public keys and republish them serially.
    async fn run_serially(
        &self,
        public_keys: Vec<PublicKey>,
    ) -> Result<HashMap<PublicKey, Result<RepublishInfo, RepublishError>>, BuildError> {
        let mut results: HashMap<PublicKey, Result<RepublishInfo, RepublishError>> =
            HashMap::with_capacity(public_keys.len());
        tracing::debug!("Start to republish {} public keys.", public_keys.len());
        // TODO: Inspect pkarr reliability.
        // pkarr client gets really unreliable when used in parallel. To get around this, we use one client per run().
        let client = self.client_builder.clone().build()?;
        let mut local_settings = self.settings.clone();
        local_settings.client = Some(client);
        for key in public_keys {
            let start = Instant::now();

            let republisher = Republisher::new_with_settings(key.clone(), local_settings.clone())
                .expect("infalliable");
            let res = republisher.republish().await;

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

    pub async fn run(
        &self,
        public_keys: Vec<PublicKey>,
        thread_count: u8,
    ) -> Result<HashMap<PublicKey, Result<RepublishInfo, RepublishError>>, BuildError> {
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
            let result = handle.await.expect("should have result")?;
            for entry in result {
                results.insert(entry.0, entry.1);
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU8;

    use pkarr::{dns::Name, ClientBuilder, Keypair, PublicKey};
    use pubky_testnet::Testnet;

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
        let testnet = Testnet::run().await.unwrap();
        // Create testnet pkarr builder
        let mut pkarr_builder = ClientBuilder::default();
        pkarr_builder.bootstrap(&testnet.bootstrap()).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();

        let public_keys = publish_sample_packets(&pkarr_client, 1).await;
        let public_key = public_keys.first().unwrap().clone();

        let mut settings = RepublisherSettings::new();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(1).unwrap());
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
        let testnet = Testnet::run().await.unwrap();
        // Create testnet pkarr builder
        let mut pkarr_builder = ClientBuilder::default();
        pkarr_builder.bootstrap(&testnet.bootstrap()).no_relays();
        let pkarr_client = pkarr_builder.clone().build().unwrap();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let mut settings = RepublisherSettings::new();
        settings
            .pkarr_client(pkarr_client)
            .min_sufficient_node_publish_count(NonZeroU8::new(2).unwrap());
        let publisher = MultiRepublisher::new_with_settings(settings, Some(pkarr_builder));
        let results = publisher.run_serially(public_keys).await.unwrap();
        let result = results.get(&public_key).unwrap();
        assert!(result.is_err());
    }
}
