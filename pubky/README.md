# Pubky

Rust implementation implementation of [Pubky](https://github.com/pubky/pubky-core) client.

## Quick Start

```rust
use pubky::Keypair;
use pubky_testnet::Testnet;

#[tokio::main]
async fn main () {
    // Mainline Dht testnet network
    let testnet = Testnet::run().await.unwrap();

    // Create and run a Homeserver.
    let server = testnet.run_homeserver().await.unwrap();

    // Create a Pubky Client from the testnet
    let client = testnet.client_builder().build().unwrap();

    // Signup to a Homeserver
    let keypair = Keypair::random();
    client.signup(&keypair, &server.public_key()).await.unwrap();

    // Write data
    let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
    let url = url.as_str();

    client
        .put(url)
        .body([0, 1, 2, 3].to_vec())
        .send()
        .await
        .unwrap();

    // Read using a Public key based link
    let response = client.get(url).send().await.unwrap();
    let blob = response.bytes().await.unwrap();
    assert_eq!(blob.to_vec(), vec![0, 1, 2, 3]);


    // Delete an entry.
    client.delete(url).send().await.unwrap();

    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.status(), 404);
}
```

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) for using the Pubky client.
