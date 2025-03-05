use std::collections::HashMap;
use pkarr::{Client, PublicKey};
use tokio::time::Instant;

use crate::single_key_publisher::{RepublishError, RepublishInfo, SingleKeyRepublisherBuilder};


pub struct PkarrRepublisher {
    results: HashMap<PublicKey, Option<Result<RepublishInfo, RepublishError>>>,
    client: pkarr::Client
}


impl PkarrRepublisher {
    pub fn new(public_keys: Vec<PublicKey>) -> Result<Self, pkarr::errors::BuildError> {
        let client = pkarr::Client::builder().build()?;
        let mut results: HashMap<PublicKey, Option<Result<RepublishInfo, RepublishError>>> = HashMap::with_capacity(public_keys.len());
        for key in public_keys {
            results.insert(key, None);
        };
        Ok(Self {
            results,
            client,
        })
    }

    pub fn new_with_client(public_keys: Vec<PublicKey>, client: Client) -> Self {
        let mut results: HashMap<PublicKey, Option<Result<RepublishInfo, RepublishError>>> = HashMap::with_capacity(public_keys.len());
        for key in public_keys {
            results.insert(key, None);
        };
        Self {
            results,
            client,
        }
    }

    /// Number of keys that haven't been checked yet
    /// either because the publisher didn't get there yet or because there was an error.
    pub fn unprocessed_keys(&self) -> Vec<PublicKey> {
        self.results.iter().filter(|(_, result)| result.is_none()).map(|(key, _)| key.clone()).collect()
    }

    /// Wait until the DHT is bootstrapped.
    /// Use this method to be sure that the DHT is ready to use.
    pub async fn wait_until_dht_is_bootstrap(&self) {
        let dht = self.client.dht().unwrap();
        dht.clone().as_async().bootstrapped().await;
    }

    /// Go through the list of all public keys and republish them.
    pub async fn run(&mut self) -> &HashMap<PublicKey, Option<Result<RepublishInfo, RepublishError>>>{
        let keys = self.unprocessed_keys();
        tracing::info!("Start to republish {} public keys. {} to go.", self.results.len(), keys.len());
        for key in keys {
            let start = Instant::now();

            let republisher = SingleKeyRepublisherBuilder::new(key.clone()).pkarr_client(self.client.clone()).build().unwrap();
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

            self.results.insert(key.clone(), Some(res));
        }
        &self.results
    }
}



#[cfg(test)]
mod tests {
    use pkarr::{dns::Name, Keypair, PublicKey};
    use pubky_testnet::Testnet;

    async fn publish_sample_packets(client: &pkarr::Client, count: usize) -> Vec<PublicKey> {
        let keys: Vec<Keypair> = (0..count).map(|_| Keypair::random()).collect();
        for key in keys.iter() {
            let packet = pkarr::SignedPacketBuilder::default().cname(Name::new("test").unwrap(), Name::new("test2").unwrap(), 600).build(key).unwrap();
            let _ = client.publish(&packet, None).await;
        };

        keys.into_iter().map(|key| key.public_key()).collect()
    }

    #[tokio::test]
    async fn single_key_republish() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let public_keys = publish_sample_packets(&pkarr_client, 1).await;

        let public_key = public_keys.first().unwrap().clone();

        let publisher = super::PkarrRepublisher::new_with_client(public_keys, pkarr_client.clone());
        let results = publisher.run().await;
        let result = results.get(&public_key).unwrap();

        if let RepublishState::Republished{..} = new_state {
            assert!(true);
        } else {
            panic!("Expected Republished state");
        }
    }

    /// Publish a single key that has not been published previously
    #[tokio::test]
    async fn single_key_missing() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();

        let public_key = Keypair::random().public_key();

        let publisher = super::PkarrRepublisher::new_with_client(vec![public_key.clone()], pkarr_client.clone());
        let new_state = publisher.republish_single_key(public_key.clone()).await.expect("Should work without an error");

        if let RepublishState::Missing = new_state {
            assert!(true);
        } else {
            panic!("Expected Missing state");
        }
    }

    /// Go through all keys and try to publish them
    #[tokio::test]
    async fn republish_keys_once() {
        let testnet = Testnet::run().await.unwrap();
        let pubky_client = testnet.client_builder().build().unwrap();
        let pkarr_client = pubky_client.pkarr().clone();
        let mut public_keys = publish_sample_packets(&pkarr_client, 10).await;
        public_keys.push(Keypair::random().public_key()); // Add key that is not published.

        let mut publisher = super::PkarrRepublisher::new_with_client(public_keys, pkarr_client.clone());
        publisher.run().await;
        assert_eq!(publisher.num_unprocessed_keys(), 0);

        for key in publisher.public_keys[..10].iter() {
            assert!(key.state.is_some());
            let state = key.state.clone().unwrap();
            assert!(state.is_republished());
        }
        for key in publisher.public_keys[10..].iter() {
            assert!(key.state.is_some());
            let state = key.state.clone().unwrap();
            assert!(state.is_missing());
        }
    }

}
