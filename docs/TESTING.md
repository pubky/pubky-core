# Testing

This guide is for contributors running Rust tests, integration tests, or CI jobs. For local app development with a testnet, see [Local Development](./LOCAL_DEVELOPMENT.md). For standalone homeserver operation, see [Install and Run Pubky Homeserver](./INSTALL.md).

## PostgreSQL for Tests

Many homeserver and testnet tests need PostgreSQL. See [Local Development - Set Up PostgreSQL](./LOCAL_DEVELOPMENT.md#set-up-postgresql) for setup options.

Run tests with a test connection string:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://localhost:5432/postgres?pubky-test=true' \
  cargo test -p pubky-homeserver --all-features
```

The `?pubky-test=true` parameter tells the test helpers to create an ephemeral `pubky_test_*` database inside the configured PostgreSQL instance. Databases are cleaned up after each test.

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

## Docker PostgreSQL in Tests

The `docker-postgres` feature lets tests automatically start a PostgreSQL container via Docker, removing the need for an external postgres instance. It's used by the [Rust examples](../examples/rust) (enabled by default) and the pubky-testnet integration tests. It's a good option for self-contained tests or CI environments where you don't want to manage a separate database.

Enable it in your crate:

```toml
[dev-dependencies]
pubky-testnet = { version = "0.x", features = ["docker-postgres"] }
```

### Per-test container

Each `.with_docker_postgres()` call starts a separate PostgreSQL container. Simple but expensive for large test suites:

```rust
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
async fn my_test() {
    let testnet = EphemeralTestnet::builder()
        .with_docker_postgres()
        .build()
        .await
        .unwrap();

    let homeserver = testnet.homeserver_app();
}
```

### Shared container

For many tests, share one Docker PostgreSQL instance with `DockerPostgres::shared()`. Each testnet still creates its own ephemeral database, so test data remains isolated:

```rust
use pubky_testnet::docker_postgres::DockerPostgres;
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
async fn test_one() {
    let pg = DockerPostgres::shared().await;
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
TEST_PUBKY_CONNECTION_STRING='postgres://localhost:5432/postgres?pubky-test=true' \
  cargo test -p pubky-homeserver --all-features
```

Run the testnet tests with Docker PostgreSQL:

```bash
cargo test -p pubky-testnet --features docker-postgres
```

Run the full workspace test suite:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://localhost:5432/postgres?pubky-test=true' \
  cargo test --workspace --all-features
```

## Troubleshooting

### PostgreSQL Connection Refused

Make sure PostgreSQL is running and listening on the host and port in `TEST_PUBKY_CONNECTION_STRING`. See [Local Development - Troubleshooting](./LOCAL_DEVELOPMENT.md#postgresql-connection-refused).

### Stale Test Databases

Test databases are named `pubky_test_*`. If a test process is killed before cleanup, drop stale databases manually with your PostgreSQL tooling.
