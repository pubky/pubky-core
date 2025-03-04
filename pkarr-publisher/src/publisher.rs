use std::any;

use pkarr::{mainline::Dht, Client, PublicKey};

#[derive(Debug, Clone)]
pub enum PublishState {
    /// The packet has been republished.
    Republished {
        published_nodes_count: usize
    },
    /// The packet can't be resolved on the DHT and therefore can't be republished.
    Missing,
}

impl PublishState {
    pub fn is_republished(&self) -> bool {
        if let PublishState::Republished{..} = self {
            return true
        } else {
            return false
        }
    }

    pub fn is_missing(&self) -> bool {
        if let PublishState::Missing = self {
            return true
        } else {
            return false
        }
    }
}


/// A struct that holds the public key and
/// the information if it is published and verified.
#[derive(Debug, Clone)]
pub struct PublicKeyPub {
    pub public_key: PublicKey,
    state: Option<PublishState>,
}

impl PublicKeyPub {
    pub fn new(public_key: PublicKey) -> Self {
        Self {
            public_key,
            state: None
        }
    }

    /// If this key has already been processed.
    pub fn is_processed(&self) -> bool {
        self.state.is_some()
    }
}

pub struct PkarrPublisher {
    pub public_keys: Vec<PublicKeyPub>,
    client: pkarr::Client,
    dht: Dht
}


impl PkarrPublisher {
    pub fn new(public_keys: Vec<PublicKey>) -> Result<Self, pkarr::errors::BuildError> {
        let client = pkarr::Client::builder().build()?;
        let dht = client.dht().unwrap();
        let keys = public_keys.into_iter().map(|pk| PublicKeyPub::new(pk)).collect();
        Ok(Self {
            public_keys: keys,
            client,
            dht
        })
    }

    pub fn new_with_client(public_keys: Vec<PublicKey>, client: Client) -> Self {
        let keys = public_keys.into_iter().map(|pk| PublicKeyPub::new(pk)).collect();
        let dht = client.dht().unwrap();
        Self {
            public_keys: keys,
            client,
            dht,
        }
    }

    /// Number of keys that haven't been checked yet
    /// either because the publisher didn't get there yet or because there was an error.
    pub fn num_unprocessed_keys(&self) -> usize {
        self.public_keys.iter().filter(|key| !key.is_processed()).count()
    }

    /// Wait until the DHT is bootstrapped.
    /// Use this method to be sure that the DHT is ready to use.
    pub async fn wait_until_dht_is_bootstrap(&self) {
        let dht = self.client.dht().unwrap();
        dht.clone().as_async().bootstrapped().await;
    }

    /// Count the number of nodes that public key is published on.
    async fn count_published_nodes(&self, public_key: &PublicKey) -> usize {
        let dht = self.dht.clone();
        let pubkey = public_key.clone();
        let count = tokio::task::spawn_blocking(move || {
            let stream = dht.get_mutable(pubkey.as_bytes(), None, None);
            let items_count = stream.map(|_| 1).reduce(|a, b| a + b);
            items_count.unwrap_or(0)
        }).await.unwrap_or(0);
        count
    }

    /// Republish a single public key.
    async fn republish_single_key(&self, public_key: PublicKey) -> Result<PublishState, pkarr::errors::PublishError> {
        let packet = self.client.resolve_most_recent(&public_key).await;
        if packet.is_none() {
            tracing::debug!("Packet {} is missing on the DHT.", public_key);
            return Ok(PublishState::Missing)
        }
        let packet = packet.unwrap();
        if let Err(e) = self.client.publish(&packet, None).await {
            return Err(e);
        }
        let published_nodes_count = self.count_published_nodes(&public_key).await;
        Ok(PublishState::Republished {published_nodes_count})
    }

    /// Go through the list of all public keys and republish them.
    async fn republish_keys_once(&mut self) {
        let mut keys = self.public_keys.clone();
        tracing::debug!("Start to republish {} public keys. {} to go.", self.public_keys.len(), self.num_unprocessed_keys());
        for key in keys.iter_mut() {
            if key.is_processed() {
                continue;
            }
            let public_key = key.public_key.clone();
            let new_state_result = self.republish_single_key(public_key.clone()).await;
            if let Err(e) = new_state_result {
                tracing::warn!("Failed to republish public_key {}. {}", public_key, e);
                continue;
            }
            key.state = Some(new_state_result.unwrap());
        } 
        self.public_keys = keys;
    }
}



#[cfg(test)]
mod tests {
    use pkarr::{dns::Name, Keypair, PublicKey};
    use pubky_testnet::Testnet;
    use super::PublishState; 

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

        let publisher = super::PkarrPublisher::new_with_client(public_keys, pkarr_client.clone());
        let new_state = publisher.republish_single_key(public_key.clone()).await.expect("Should work without an error");

        if let PublishState::Republished{published_nodes_count} = new_state {
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

        let publisher = super::PkarrPublisher::new_with_client(vec![public_key.clone()], pkarr_client.clone());
        let new_state = publisher.republish_single_key(public_key.clone()).await.expect("Should work without an error");

        if let PublishState::Missing = new_state {
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

        let mut publisher = super::PkarrPublisher::new_with_client(public_keys, pkarr_client.clone());
        publisher.republish_keys_once().await;
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
