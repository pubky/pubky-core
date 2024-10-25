# Pubky

Rust implementation implementation of [Pubky](https://github.com/pubky/pubky-core) client.

## Quick Start

```rust
use pkarr::mainline::Testnet;
use pkarr::Keypair;
use pubky_homeserver::Homeserver;
use pubky::PubkyClient;

#[tokio::main]
async fn main () {
  // Mainline Dht testnet and a temporary homeserver for unit testing.
  let testnet = Testnet::new(10);
  let server = Homeserver::start_test(&testnet).await.unwrap();

  let client = PubkyClient::test(&testnet);

  // Uncomment the following line instead if you are not just testing.
  // let client PubkyClient::builder().build(); 

  // Generate a keypair
  let keypair = Keypair::random();

  // Signup to a Homeserver
  let keypair = Keypair::random();
  client.signup(&keypair, &server.public_key()).await.unwrap();

  // Write data.
  let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
  let url = url.as_str();

  client.put(url, &[0, 1, 2, 3, 4]).await.unwrap();

  // Read using a Public key based link
  let response = client.get(url).await.unwrap().unwrap();

  assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

  // Delet an entry.
  client.delete(url).await.unwrap();

  let response = client.get(url).await.unwrap();

  assert_eq!(response, None);
}
```

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) for using the Pubky client.
