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

```rust,no_run
# #[cfg(not(feature = "embedded-postgres"))]
# fn main() {}
# #[cfg(feature = "embedded-postgres")]
use pubky_testnet::EphemeralTestnet;

# #[cfg(feature = "embedded-postgres")]
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

> **Important**: If you have multiple tests, see [Sharing Embedded Postgres Across Tests](#sharing-embedded-postgres-across-tests) below.

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

## Sharing Embedded Postgres Across Tests

When using `embedded-postgres`, each call to `.with_embedded_postgres()` starts a **separate** PostgreSQL instance.

Use [`EmbeddedPostgres::shared()`] to start **one** instance and share its connection string across all tests.
This method registers an atexit handler that stops the PostgreSQL child process when the test process exits,
preventing orphaned processes.

```rust
use pubky_testnet::EphemeralTestnet;
use pubky_testnet::embedded_postgres::EmbeddedPostgres;

#[tokio::test]
async fn test_one() {
    let pg = EmbeddedPostgres::shared().await;
    let testnet = EphemeralTestnet::builder()
        .postgres(pg.connection_string().unwrap())
        .build()
        .await
        .unwrap();
    // ... test code
}

#[tokio::test]
async fn test_two() {
    let pg = EmbeddedPostgres::shared().await;
    let testnet = EphemeralTestnet::builder()
        .postgres(pg.connection_string().unwrap())
        .build()
        .await
        .unwrap();
    // ... test code
}
```

Each testnet still gets its own ephemeral database within the shared PostgreSQL instance, so tests remain isolated.

## Troubleshooting

### GitHub Rate Limiting During Binary Download

The embedded PostgreSQL binary is downloaded from GitHub releases. If multiple tests (or repeated test runs) try to download concurrently, you may hit GitHub's API rate limit (60 requests/hour for unauthenticated requests), causing errors like `403 Forbidden` or `rate limit exceeded`.

**Solutions (try in order):**

1. **Set a GitHub token** to raise the rate limit from 60 to 5,000 requests/hour:
   ```bash
   export GITHUB_TOKEN=ghp_your_personal_access_token
   cargo test
   ```
   The token does not need any scopes — a classic PAT with no permissions works.

2. **Run a single test first** to populate the cache before running the full suite.

3. **Share one embedded postgres instance** across tests (see [Sharing Embedded Postgres Across Tests](#sharing-embedded-postgres-across-tests)).

4. **Wait for the rate limit to reset** (1 hour from first rate-limited request), then retry with one of the above solutions.

**Cache location:** `~/.cache/pubky-testnet/postgresql/` (Linux/macOS). If you suspect a corrupt cache, delete this directory and retry.

## Binary (Static Testnet)

If you need to run the testnet in a separate process (e.g., to test Pubky Core in browsers), run the binary which creates these components with hardcoded configurations:

1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
2. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
3. A Homeserver with address `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
