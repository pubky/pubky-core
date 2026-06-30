# Local Development

This guide helps you set up a local Pubky network for development, whether you're building an app on top of Pubky, contributing to the homeserver, or working on other crates in this repo.

If you don't need to build from source or control individual components, see the [Docker Compose quick start](./INSTALL.md#quick-start-with-docker-compose) in the Install guide for the fastest way to get a homeserver running.

For deploying a standalone homeserver, see [Install and Run Pubky Homeserver](./INSTALL.md). For running Rust tests and CI, see [Testing](./TESTING.md).

## Contents

- [Toolchain](#toolchain)
- [Ephemeral Testnet](#ephemeral-testnet)
  - [Set Up PostgreSQL](#set-up-postgresql)
  - [Run the Testnet](#run-the-testnet)
- [Run Examples](#run-examples) (keygen, signup, write, read)
- [Custom Homeserver Config](#custom-homeserver-config)
- [Troubleshooting](#troubleshooting)

## Toolchain

For building from source and running the ephemeral testnet:

- Rust `1.89` or newer.
- PostgreSQL for tests and the ephemeral testnet (see [Set Up PostgreSQL](#set-up-postgresql)).
- Node.js `20` or newer for JS/WASM bindings.
- `wasm-pack` when working on the JavaScript SDK bindings.

Useful commands:

```bash
cargo check --workspace --all-features
cargo fmt --check
cargo clippy --workspace --all-features --exclude pubky-wasm -- -D warnings
```


## Ephemeral Testnet

The ephemeral testnet starts a full local Pubky network from source: Homeserver, local DHT and relays. Data is cleaned up when the process exits.

### Set Up PostgreSQL

The testnet requires a running PostgreSQL instance with a user that can create databases. You do **not** need to create a database manually, the testnet creates ephemeral `pubky_test_*` databases automatically and cleans them up on exit.

The quickest option is Docker:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -p 127.0.0.1:5432:5432 \
  -d postgres:18
```

For native PostgreSQL or an existing instance, see [INSTALL.md - Set Up PostgreSQL](./INSTALL.md#set-up-postgresql). Skip the database creation step if youre only using testnet.

### Run the Testnet

Using the Docker PostgreSQL from above:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet
```

Replace the connection string if your PostgreSQL instance uses different credentials or host.

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

With the testnet running, use the examples in a separate terminal. Rust examples are in [`examples/rust`](../examples/rust).

### 1. Generate a keypair

```bash
cargo run -p pubky-core-examples --bin keygen -- --output /tmp/pubky-recovery.json
```

You will be prompted to set and confirm a passphrase. Note the public key printed in the output.

### 2. Generate a signup token

```bash
curl "http://127.0.0.1:6288/generate_signup_token" \
  -H "X-Admin-Password: admin"
```

Copy the token from the response.

### 3. Sign up

```bash
cargo run -p pubky-core-examples --bin signup -- --testnet \
  pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo \
  /tmp/pubky-recovery.json \
  <SIGNUP_TOKEN>
```

Replace `<SIGNUP_TOKEN>` with the token from step 2. You will be prompted for your passphrase.

### 4. Write data

```bash
cargo run -p pubky-core-examples --bin storage -- write --testnet \
  /tmp/pubky-recovery.json \
  /pub/example/hello.txt \
  --content "Hello from pubky!"
```

This signs in, writes the file, reads it back, and deletes it.


### Quick smoke test

For a self-contained test (requires Docker) that starts its own ephemeral testnet, signs up a user, writes a file, and reads it back:

```bash
cargo run -p pubky-core-examples --bin testnet
```

JavaScript examples are in [`examples/javascript`](../examples/javascript). They use the local testnet defaults when configured for testnet mode.

### JavaScript / WASM Bindings

To build and test the JavaScript SDK bindings:

```bash
cd pubky-sdk/bindings/js/pkg
npm i
npm run build
```

To run the JS binding tests, start the testnet first (see [Run the Testnet](#run-the-testnet)), then in a separate terminal:

```bash
cd pubky-sdk/bindings/js/pkg
npm test
```

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
