# Pubky

Rust implementation of [Pubky](https://github.com/pubky/pubky-core) client.

## Quick Start

```rust
use pubky_testnet::EphemeralTestnet;
use pubky::Keypair;

#[tokio::main]
async fn main () {
  // Mainline Dht testnet and a temporary homeserver for unit testing.
  let testnet = EphemeralTestnet::start().await.unwrap();
  let client = testnet.pubky_client().unwrap();

  let homeserver = testnet.homeserver_suite();

  // Generate a keypair
  let keypair = Keypair::random();

  // Signup to a Homeserver
  let keypair = Keypair::random();
  client.signup(&keypair, &homeserver.public_key(), None).await.unwrap();

  // Write data.
  let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
  let url = url.as_str();

  let data = [0, 1, 2, 3, 4].to_vec();

  // The client has the same familiar API of a reqwest client
  client.put(url).body(data.clone()).send().await.unwrap();

  // Read using a Public key based link
  let response = client.get(url).send().await.unwrap();
  let response_data = response.bytes().await.unwrap();

  assert_eq!(response_data, data);

  // Delete an entry.
  client.delete(url).send().await.unwrap();

  let response = client.get(url).send().await.unwrap();

  assert_eq!(response.status(), 404);
}
```

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) for using the Pubky client.

## JS bindings

Find a wrapper of this crate using `wasm_bindgen` in `pubky-client/bindings/js`
