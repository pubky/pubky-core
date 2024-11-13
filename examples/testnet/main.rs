//! Example of using a Testnet configuration in pubky Client

use pubky::PubkyClient;
use pubky_common::crypto::{Keypair, PublicKey};

#[tokio::main]
async fn main() {
    let server_public_key =
        PublicKey::try_from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo").unwrap();

    let client = PubkyClient::testnet();

    let keypair = Keypair::random();

    client.signup(&keypair, &server_public_key).await.unwrap();

    let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
    let url = url.as_str();

    client.put(url, &[0, 1, 2, 3, 4]).await.unwrap();

    let response = client.get(url).await.unwrap().unwrap();

    assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

    client.delete(url).await.unwrap();

    let response = client.get(url).await.unwrap();

    assert_eq!(response, None);

    println!("Successfully performed PUT, GET and DELETE requests to the testnet server")
}
