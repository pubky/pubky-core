# Pubky

Rust implementation implementation of [Pubky](https://github.com/pubky/pubky-core) client.

## Quick Start

```rust
use pubky_testnet::Testnet;
use pubky::{Client, Keypair};

#[tokio::main]
async fn main () {
  // Mainline Dht testnet and a temporary homeserver for unit testing.
  let testnet = Testnet::run_with_hardcoded_configurations().await.unwrap();
  let homeserver = testnet.run_homeserver().await.unwrap();

  let client = Client::builder().testnet().build().unwrap();

  // Uncomment the following line instead if you are not just testing.
  // let client Client::builder().build().unwrap();

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

## Wasm Rust Analyzer

In vscode with the rust-analyzer, wasm behind the `#[cfg(wasm_browser)]` guard is not type checked. To fix this, add 
a `.vscode/settings.json` file in the root of this project with the following content:

```json
{
    "rust-analyzer.cargo.target": "wasm32-unknown-unknown"
}
```

If not done already, you need to add the wasm target: `cargo target add wasm32-unknown-unknown`.

This is just a workaround because it enables the wasm feature in all workspace member which creates problems.
So it is best to enable this settings only temporarily for wasm development and then turn it off again before commiting the
changes. This is a [rust-analyzer issue](https://github.com/rust-lang/rust-analyzer/issues/11900#issuecomment-1166638234).

## How To Build/Test the NPM Package

1. Go to `pubky/pkg`.
2. Run `npm run build`.
3. Run a testnet mainline DHT, Pkarr relay and Homeserver `npm run testnet`
4. Run tests with `npm run test`.