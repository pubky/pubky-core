# Testing

This guide is for contributors running Rust tests, integration tests, or CI jobs. For local app development with a long-lived testnet, see [Local Development](./LOCAL_DEVELOPMENT.md). For standalone homeserver operation, see [Install and Run Pubky Homeserver](./INSTALL.md).

## PostgreSQL for Tests

Many homeserver and testnet tests need PostgreSQL. The easiest external database setup is Docker:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=postgres \
  -p 127.0.0.1:5432:5432 \
  -d postgres:18-alpine
```

Then run tests with a test connection string:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo test -p pubky-homeserver --all-features
```

The `?pubky-test=true` parameter marks the URL as a test database URL. The test helpers create an ephemeral database named `pubky_test_*` inside the configured PostgreSQL instance.

## Test Database Cleanup

Use the Pubky test macro for tests that create ephemeral PostgreSQL databases:

```rust
#[tokio::test]
#[pubky_testnet::test]
async fn my_test() {
    // test code
}
```

The macro ensures registered test databases are dropped after the test completes or panics.

## Embedded PostgreSQL in Tests

Enable the `embedded-postgres` feature when you want tests to run without a separate PostgreSQL installation:

```toml
[dev-dependencies]
pubky-testnet = { version = "0.7", features = ["embedded-postgres"] }
```

Then build testnets with embedded PostgreSQL:

```rust
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
async fn my_test() {
    let testnet = EphemeralTestnet::builder()
        .with_embedded_postgres()
        .build()
        .await
        .unwrap();

    let homeserver = testnet.homeserver_app();
}
```

Each call to `.with_embedded_postgres()` starts a separate PostgreSQL server. That is convenient for a small number of tests but expensive for large suites.

## Share Embedded PostgreSQL Across Tests

For many tests, start one embedded PostgreSQL instance and pass its connection string to each testnet. Each testnet still creates its own ephemeral database, so test data remains isolated.

```rust
use pubky_testnet::embedded_postgres::EmbeddedPostgres;
use pubky_testnet::EphemeralTestnet;
use tokio::sync::OnceCell;

static SHARED_PG: OnceCell<EmbeddedPostgres> = OnceCell::const_new();

async fn shared_postgres() -> &'static EmbeddedPostgres {
    SHARED_PG
        .get_or_init(|| async {
            EmbeddedPostgres::start()
                .await
                .expect("Failed to start embedded postgres")
        })
        .await
}

#[tokio::test]
async fn test_one() {
    let pg = shared_postgres().await;
    let testnet = EphemeralTestnet::builder()
        .postgres(pg.connection_string().unwrap())
        .build()
        .await
        .unwrap();

    let homeserver = testnet.homeserver_app();
}
```

## Common Commands

Run the homeserver tests against external PostgreSQL:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo test -p pubky-homeserver --all-features
```

Run the testnet tests with embedded PostgreSQL enabled:

```bash
cargo test -p pubky-testnet --features embedded-postgres
```

Run the full workspace test suite:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo test --workspace --all-features
```

## Troubleshooting

### PostgreSQL Connection Refused

Make sure PostgreSQL is running and listening on the host and port in `TEST_PUBKY_CONNECTION_STRING`.

For Docker:

```bash
docker ps --filter name=pubky-postgres
```

### Stale Test Databases

Test databases are named `pubky_test_*`. If a test process is killed before cleanup, drop stale databases manually with your PostgreSQL tooling.

### Embedded PostgreSQL Download Rate Limited

The embedded PostgreSQL binary is downloaded from GitHub releases. If you hit API limits, set a GitHub token:

```bash
export GITHUB_TOKEN=ghp_your_personal_access_token
cargo test -p pubky-testnet --features embedded-postgres
```

The token does not need repository permissions.

### Corrupt Embedded PostgreSQL Cache

Remove the embedded PostgreSQL cache and rerun the tests:

```bash
rm -rf ~/.cache/pubky-testnet/postgresql
```
