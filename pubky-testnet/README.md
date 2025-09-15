# Pubky Testnet

A local test network for developing Pubky Core or applications depending on it.

All resources are ephemeral, the database is an empheral Postgres, and all servers are cleaned up as the testnet dropped.

## Usage

### Postgres

For the homeserver and therefore this testnet to be used, a postgres server is required. 
By default, testnet will use `postgres://localhost:5432/postgres?pubky-test=true`.
`?pubky_test=true` indicates that the homeserver should create an emphemeral database.

If you want to change the [connection string](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING-URIS) you have 2 options.

- Set the `TEST_PUBKY_CONNECTION_STRING` environment variable.
- Set the connection string in the testnet constructor.

```rust
let connection_string = ConnectionString::new("postgres://localhost:5432/my_db").unwrap();
let testnet = Testnet::new_with_custom_postgres(connection_string).await.unwrap();
```

### Inline testing

```rust
use pubky_testnet::EphemeralTestnet;

#[tokio::main]
#[pubky_testnet::test] // Makro makes sure that the empheral Postgres databases are cleaned up.
async fn main () {
  // Run a new testnet. This creates a test dht,
  // a homeserver, and a http relay.
  let testnet = EphemeralTestnet::start().await.unwrap();

  // Create a Pubky Client from the testnet.
  let client = testnet.pubky_client().unwrap();

  // Use the homeserver
  let homeserver = testnet.homeserver_suite();

  // Use the relay
  let http_relay = testnet.http_relay();
}
```

### Binary (hardcoded testnet, and browser support).

If you need to run the testnet in a separate process, for example to test Pubky Core in browsers, you need to run this binary, which will create these components with hardcoded configurations:

1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
2. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
3. A Homeserver with address is hardcoded to `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
