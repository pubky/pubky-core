# Pubky Testnet

A local test network for developing Pubky Core or applications depending on it.

Two testnet types are provided:

| Type | Ports | Storage | Use case |
|------|-------|---------|----------|
| [`EphemeralTestnet`] | Random | In-memory | Automated tests (`#[tokio::test]`) - parallel-safe, no port conflicts |
| [`StaticTestnet`] | Fixed, well-known | In-memory or persistent | Interactive / CLI use - browser tests, mobile apps, manual debugging |
For running a long-lived local testnet as a separate process, see the [local development guide](../docs/LOCAL_DEVELOPMENT.md). This README focuses on the `pubky-testnet` crate API.


## Table of Contents

- [Prerequisites](#prerequisites)
- [EphemeralTestnet (Automated Tests)](#ephemeraltestnet-automated-tests)
  - [Writing Tests](#writing-tests)
  - [Docker PostgreSQL](#docker-postgresql)
  - [Sharing Docker Postgres Across Tests](#sharing-docker-postgres-across-tests)
  - [Custom Configuration](#custom-configuration)
- [StaticTestnet (CLI / Interactive)](#statictestnet-cli--interactive)
  - [Fixed Ports](#fixed-ports)
  - [In-Memory Mode](#in-memory-mode)
  - [Persistent Mode](#persistent-mode)
  - [Custom Homeserver Config](#custom-homeserver-config)
- [Troubleshooting](#troubleshooting)

## Prerequisites

All testnet modes require a PostgreSQL database. You can either:

- **Use Docker Postgres** (recommended for tests) — enable the `docker-postgres` feature, no external setup needed.
- **Run your own Postgres** — set the `TEST_PUBKY_CONNECTION_STRING` environment variable.

```bash
# Example: start a local Postgres container
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=pubky_homeserver \
  -p 127.0.0.1:5432:5432 \
  -d postgres:18-alpine
```

The `TEST_PUBKY_CONNECTION_STRING` environment variable is used by both testnet types to configure the database connection.

## EphemeralTestnet (Automated Tests)

All ports are random, all state is in-memory. Each instance gets its own isolated DHT and homeserver, so tests run in parallel without conflicts.

### Writing Tests

```rust,no_run
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
#[pubky_testnet::test] // Cleans up ephemeral Postgres databases after the test
async fn my_test() {
    // Note: both attributes are required — #[tokio::test] provides the async
    // runtime, #[pubky_testnet::test] registers a cleanup hook for test DBs.
    // Run a new testnet. This creates a test DHT and homeserver.
    // By default, uses minimal_test_config() (admin/metrics disabled, no HTTP relay).
    let testnet = EphemeralTestnet::builder().build().await.unwrap();

    // Create a Pubky Http Client from the testnet.
    let client = testnet.client().unwrap();

    // Use the homeserver
    let homeserver = testnet.homeserver_app();
}
```

### Docker PostgreSQL

For testing without a separate Postgres installation, enable the `docker-postgres` feature:

```toml
[dev-dependencies]
pubky-testnet = { version = "0.9", features = ["docker-postgres"] }
```

```rust,no_run
# #[cfg(not(feature = "docker-postgres"))]
# fn main() {}
# #[cfg(feature = "docker-postgres")]
use pubky_testnet::EphemeralTestnet;

# #[cfg(feature = "docker-postgres")]
#[tokio::main]
async fn main() {
    let testnet = EphemeralTestnet::builder()
        .with_docker_postgres()
        .build()
        .await
        .unwrap();
}
```

This uses [testcontainers](https://docs.rs/testcontainers) to run PostgreSQL in a Docker container.
Docker must be running on the host. The container is automatically cleaned up on drop and on Ctrl+C/SIGTERM.

> **Important**: If you have multiple tests, see [Sharing Docker Postgres Across Tests](#sharing-docker-postgres-across-tests) below.

### Option 2: External PostgreSQL

If you prefer to use an external Postgres instance:

```bash
# Example local Postgres with password auth
docker run --name postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=postgres \
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

By default (without docker-postgres), testnet will use `postgres://localhost:5432/postgres?pubky-test=true`.
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

## Sharing Docker Postgres Across Tests

When using `docker-postgres`, each call to `.with_docker_postgres()` starts a **separate** PostgreSQL container.

Use `DockerPostgres::shared()` to start **one** container and share its connection string across all tests.
Docker handles cleanup automatically when the process exits.

```rust
# #[cfg(feature = "docker-postgres")]
# mod docker_postgres_example {
use pubky_testnet::EphemeralTestnet;
use pubky_testnet::docker_postgres::DockerPostgres;

#[tokio::test]
async fn test_one() {
    let pg = DockerPostgres::shared().await;
    let testnet = EphemeralTestnet::builder()
        .postgres(pg.connection_string().unwrap())
        .build()
        .await
        .unwrap();
    // ... test code
}

#[tokio::test]
async fn test_two() {
    let pg = DockerPostgres::shared().await;
    let testnet = EphemeralTestnet::builder()
        .postgres(pg.connection_string().unwrap())
        .build()
        .await
        .unwrap();
    // ... test code
}
# }
```

Each testnet still gets its own ephemeral database within the shared PostgreSQL instance, so tests remain isolated.

### Custom Configuration

```rust,no_run
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

## StaticTestnet (CLI / Interactive)

A long-running testnet with fixed, well-known ports. Use this when external processes need to connect (browser tests, mobile apps, manual debugging).

### Fixed Ports

| Component | Port |
|-----------|------|
| DHT bootstrap node | `6881` |
| Pkarr relay | `15411` |
| HTTP relay | `15412` |
| Homeserver ICANN HTTP | `6286` |
| Homeserver Pubky HTTPS | `6287` |
| Homeserver admin | `6288` |

Homeserver address: `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`

### In-Memory Mode

State is lost on shutdown. Use `?pubky-test=true` to auto-create and clean up an ephemeral database:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet
```

### Persistent Mode

State survives restarts. The data directory is auto-initialized on first run with a `config.toml` and server keypair. On subsequent runs, the existing state is picked up and the homeserver keeps the same identity.

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres' \
  cargo run -p pubky-testnet -- persist ./my-testnet-data
```

The `TEST_PUBKY_CONNECTION_STRING` environment variable is read on every startup and overrides the `database_url` in the on-disk config.

> **Note**: For persistent mode, ensure `?pubky-test=true` is omitted so that the database is not cleaned up on shutdown.

### Custom Homeserver Config

Seed a custom config on first run (errors if `config.toml` already exists in the data directory):

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres' \
  cargo run -p pubky-testnet -- --homeserver-config my-config.toml persist ./my-testnet-data
```

## Troubleshooting

### Docker not running

The `docker-postgres` feature requires Docker. If you see `"Is Docker running?"` errors, ensure the Docker daemon is started and your user has permission to access it (e.g., is in the `docker` group).

### Docker Hub rate limits

The Postgres image is pulled from Docker Hub. Anonymous pulls are limited to 100 per 6 hours. If you hit this, either `docker login` or pre-pull the image:

```bash
docker pull postgres
```

Once cached locally, subsequent test runs won't pull again.
<<<<<<< HEAD
=======

## Binary (Static Testnet)

If you need to run the testnet in a separate process (e.g., to test Pubky Core in browsers), run the binary which creates these components with hardcoded configurations:

```bash
cargo run -p pubky-testnet --features embedded-postgres -- --embedded-postgres
```

1. A local DHT with bootstrapping nodes: `&["localhost:6881"]`
2. A Pkarr Relay running on port [15411](pubky_common::constants::testnet_ports::PKARR_RELAY)
3. A Homeserver with address `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`
4. An HTTP relay running on port [15412](pubky_common::constants::testnet_ports::HTTP_RELAY)
>>>>>>> 9d4cfa45 (docs: Improve readmes and add INSTALL and LOCAL_DEVELOPMENT docs)
