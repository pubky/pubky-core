# Local Development

This guide helps you set up a local Pubky network for development, whether you're building an app on top of Pubky, contributing to the homeserver, or working on other crates in this repo.

There are two local development options:

- **Ephemeral testnet** — starts a full local network from source with disposable data. Data is cleaned up when the process exits. Good for quick iteration and testing.
- **Persistent testnet (coming soon)** — a Docker Compose setup that persists data across restarts. Good for longer-lived development and app integration.

For deploying a standalone homeserver, see [Install and Run Pubky Homeserver](./INSTALL.md). For running Rust tests and CI, see [Testing](./TESTING.md).

## Contents

- [Persistent Testnet (coming soon)](#persistent-testnet-coming-soon)
- [Ephemeral Testnet](#ephemeral-testnet)
  - [Set Up PostgreSQL](#set-up-postgresql)
  - [Run the Testnet](#run-the-testnet)
- [Run Examples](#run-examples)
- [Custom Homeserver Config](#custom-homeserver-config)
- [Troubleshooting](#troubleshooting)

## Persistent Testnet (coming soon)

A `docker-compose.yml` that runs the full local network (homeserver, PostgreSQL, DHT, relays) with persistent storage is planned. This will be the easiest way to develop against a stable local environment that survives restarts.

## Ephemeral Testnet

The ephemeral testnet starts a full local Pubky network from source: Homeserver, local DHT and relays. Data is cleaned up when the process exits.

### Set Up PostgreSQL

The testnet requires a running PostgreSQL instance. The quickest option is Docker:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_HOST_AUTH_METHOD=trust \
  -e POSTGRES_DB=postgres \
  -p 127.0.0.1:5432:5432 \
  -d postgres:17
```

The testnet creates ephemeral `pubky_test_*` databases automatically, you don't need to create a database manually.

See [INSTALL.md - Set Up PostgreSQL](./INSTALL.md#set-up-postgresql) for native install or if you have an existing Postgres instance already.

### Run the Testnet

Start the testnet with the default connection string (`postgres://localhost:5432/postgres?pubky-test=true`):

```bash
cargo run -p pubky-testnet
```

Or point it at a custom PostgreSQL instance:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://<USER>:<PASSWORD>@<HOST>:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet
```

The `?pubky-test=true` parameter tells the testnet to create an ephemeral `pubky_test_*` database inside the configured PostgreSQL instance. The database is cleaned up when the testnet exits.

It starts:

| Component | Default |
| --- | --- |
| DHT bootstrap node | `127.0.0.1:6881` |
| Pkarr relay | `http://127.0.0.1:15411` |
| HTTP relay | `http://127.0.0.1:15412` |
| Homeserver HTTP API | `http://127.0.0.1:6286` |
| Homeserver Pubky TLS API | `127.0.0.1:6287` |
| Homeserver admin API | `http://127.0.0.1:6288` |

The testnet homeserver uses this public key:

```text
pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo
```

## Run Examples

With the testnet running, use the examples in a separate terminal.

Rust examples are in [`examples/rust`](../examples/rust):

```bash
cargo run -p pubky-core-examples --bin signup -- --testnet pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo /tmp/pubky-recovery.json
```

JavaScript examples are in [`examples/javascript`](../examples/javascript). They use the local testnet defaults when configured for testnet mode.

For a quick self-contained SDK smoke test, run:

```bash
cargo run -p pubky-core-examples --bin testnet
```

That example starts its own ephemeral testnet, signs up a user, writes a file, and reads it back.

## Custom Homeserver Config

The testnet binary accepts a homeserver config path:

```bash
cargo run -p pubky-testnet -- --homeserver-config ./path/to/config.toml
```

Use this when you need to test a specific homeserver configuration while still using the local DHT and relay setup.

## Troubleshooting

### Examples Cannot Connect

Make sure the testnet is still running and that the expected ports are not blocked or already used by another process.

### Port Already In Use

Stop the process using the conflicting port or use a custom homeserver config for the homeserver ports. The static DHT and relay ports are fixed for the testnet.

### PostgreSQL Connection Refused

Make sure PostgreSQL is running and listening on the expected host and port. For the Docker example above:

```bash
docker ps --filter name=pubky-postgres
```