use futures_lite::StreamExt;
use pkarr::{mainline::async_dht::AsyncDht, PublicKey};

/// Verifies the number of nodes that store the public key.
pub async fn count_key_on_dht(public_key: &PublicKey, dht: &AsyncDht) -> usize {
    let mut response_count = 0;
    let mut stream = dht.get_mutable(public_key.as_bytes(), None, None);
    while let Some(_) = stream.next().await {
        response_count += 1;
    }
    response_count
}
