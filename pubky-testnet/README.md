# Pubky Testnet

A local test network for developing Pubky Core or applications depending on it.

Two testnet types are provided:

| Type | Ports | Storage | Use case |
|------|-------|---------|----------|
| [`EphemeralTestnet`] | Random | In-memory | Automated tests (`#[tokio::test]`) — parallel-safe, no port conflicts |
| [`StaticTestnet`] | Fixed, well-known | In-memory or persistent | Interactive / CLI use - browser tests, mobile apps, manual debugging |

## Quickstart

### Option 1: Docker PostgreSQL (No External DB Required)

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

## Troubleshooting

### Docker not running

The `docker-postgres` feature requires Docker. If you see `"Is Docker running?"` errors, ensure the Docker daemon is started and your user has permission to access it (e.g., is in the `docker` group).

### Docker Hub rate limits

The Postgres image is pulled from Docker Hub. Anonymous pulls are limited to 100 per 6 hours. If you hit this, either `docker login` or pre-pull the image:

```bash
docker pull postgres
```

Once cached locally, subsequent test runs won't pull again.

## Binary (Static Testnet)

If you need to run the testnet in a separate process (e.g., to test Pubky Core in browsers), run the binary which creates a `StaticTestnet` with fixed ports:

| Component | Port |
|-----------|------|
| DHT bootstrap node | `6881` |
| Pkarr relay | `15411` |
| HTTP relay | `15412` |
| Homeserver ICANN HTTP | `6286` |
| Homeserver Pubky HTTPS | `6287` |
| Homeserver admin | `6288` |

Homeserver address: `8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo`

```bash
# In-memory (state lost on shutdown)
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet

# Persistent (state survives restarts)
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet -- persist ./my-testnet-data

# Seed a custom config on first run
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet -- --homeserver-config my-config.toml persist ./my-testnet-data
```

In persistent mode, the data directory is auto-initialized on first run with a `config.toml` and a server keypair. On subsequent runs, the existing state is picked up. The homeserver keeps the same identity across restarts.
