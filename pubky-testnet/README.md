# Pubky Testnet

A local test network for developing Pubky Core or applications depending on it.

All resources are ephemeral, including the database, and all servers are cleaned up when the testnet is dropped.

## Quickstart

### Option 1: Embedded PostgreSQL (No External DB Required)

For testing without a separate Postgres installation, enable the `embedded-postgres` feature:

```toml
[dev-dependencies]
pubky-testnet = { version = "0.6", features = ["embedded-postgres"] }
```

```rust
use pubky_testnet::EphemeralTestnet;

#[tokio::main]
async fn main() {
    let testnet = EphemeralTestnet::builder()
        .with_embedded_postgres()
        .build()
        .await
        .unwrap();
}
```

The first run will download PostgreSQL binaries (~50-100MB), which are cached for subsequent runs.

### Option 2: External PostgreSQL

If you prefer to use an external Postgres instance:

```bash
# Example local Postgres with password auth
docker run --name postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=pubky_homeserver \
  -p 127.0.0.1:5432:5432 \
  -d postgres:18-alpine
```

Then run the testnet binary:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' cargo run -p pubky-testnet
```

## Usage

### Writing Tests

```rust
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
#[pubky_testnet::test] // Macro ensures ephemeral Postgres databases are cleaned up
async fn my_test() {
    // Run a new testnet. This creates a test DHT and homeserver.
    // By default, uses minimal_test_config() (admin/metrics disabled, no HTTP relay).
    let testnet = EphemeralTestnet::builder().build().await.unwrap();

    // Create a Pubky Http Client from the testnet.
    let client = testnet.client().unwrap();

    // Use the homeserver
    let homeserver = testnet.homeserver_app();
}
```

### Custom Postgres Connection

By default (without embedded-postgres), testnet will use `postgres://localhost:5432/postgres?pubky-test=true`.
The `?pubky-test=true` parameter indicates that the homeserver should create an ephemeral database.

To use a custom [connection string](https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING-URIS):

**Option A**: Set the `TEST_PUBKY_CONNECTION_STRING` environment variable.

**Option B**: Pass the connection string programmatically:

```rust
use pubky_testnet::{Testnet, pubky_homeserver::ConnectionString};

#[tokio::main]
async fn main() {
    let connection_string = ConnectionString::new("postgres://localhost:5432/my_db").unwrap();
    let testnet = Testnet::new_with_custom_postgres(connection_string).await.unwrap();
}
```

### Custom Configuration

```rust
use pubky_testnet::{EphemeralTestnet, pubky_homeserver::ConfigToml, pubky::Keypair};

#[tokio::main]
async fn main() {
    // Enable admin server for tests that need it
    let testnet = EphemeralTestnet::builder()
        .config(ConfigToml::default_test_config())
        .build()
        .await
        .unwrap();

    // Or use a custom keypair
    let testnet = EphemeralTestnet::builder()
        .keypair(Keypair::random())
        .build()
        .await
        .unwrap();

    // Enable HTTP relay for tests that need it
    let testnet = EphemeralTestnet::builder()
        .with_http_relay()
        .build()
        .await
        .unwrap();
    let http_relay = testnet.http_relay();
}
```

## Binary (Static Testnet)

If you need to run the testnet in a separate process (e.g., to test Pubky Core in browsers), run the binary which creates these components with hardcoded configurations:

1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
2. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
3. A Homeserver with address `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
