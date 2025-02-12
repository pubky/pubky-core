# Pubky Testnet

A local test network for developing Pubky Core or applications depending on it.

All resources are ephemeral, databases are in the operating system's temporaray directories, and all servers are closed as the testnet dropped.

## Usage

### Inline testing

```rust
use pubky_testnet::Testnet;

#[tokio::main]
async fn main () {
  // Run a new testnet.
  let testnet = Testnet::run().await.unwrap();

  // Optionally create and run a Homeserver.
  let server = testnet.run_homeserver().await.unwrap();

  // Optionally create and run an HTTP Relay.
  let http_relay = testnet.run_http_relay().await.unwrap();

  // Create a Pubky Client from the testnet.
  let client = testnet.client_builder().build().unwrap();
}
```

### Binary (hardcoded testnet, and browser support).

If you need to run the testnet in a separate process, for example to test Pubky Core in browsers, you need to run this binary, which will create these components with hardcoded configurations:

1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
3. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
2. A Homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
