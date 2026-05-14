<h1 align="center"><a href="https://pubky.org/"><img alt="Pubky" src="./.svg/pubky-core-logo.svg" width="200" /></a></h1>

<h3 align="center">
  Reference homeserver implementation for Pubky.
</h3>

<div align="center">
  <h3>
    <a href="https://docs.pubky.org/">Docs</a>
    <span> | </span>
    <a href="https://docs.rs/pubky">Rust SDK</a>
    <span> | </span>
    <a href="https://www.npmjs.com/package/@synonymdev/pubky">JavaScript SDK</a>
  </h3>
</div>

[![GitHub Release](https://img.shields.io/github/v/release/pubky/pubky-homeserver)](https://github.com/pubky/pubky-homeserver/releases/latest/)
[![Crates.io Version](https://img.shields.io/crates/v/pubky)](https://crates.io/crates/pubky)
[![npm Version](https://img.shields.io/npm/v/@synonymdev/pubky)](https://www.npmjs.com/package/@synonymdev/pubky)
[![GitHub License](https://img.shields.io/github/license/pubky/pubky-homeserver)](https://github.com/pubky/pubky-homeserver/blob/main/LICENSE)

## What Is This Repository?

This repository contains `pubky-homeserver`, the reference homeserver implementation for Pubky, and the crates needed to run, test, and integrate with it.

Pubky Core is the broader protocol ecosystem: public-key identity, PKDNS/Pkarr discovery, Pubky TLS, authentication flows, and user-controlled storage. This repository focuses on the homeserver and its closely related developer tooling.

## What Is a Homeserver?

A Pubky homeserver acts as a user's agent on the internet. It provides availability for user data, exposes HTTP APIs for authenticated writes and public reads, and publishes the records that allow other clients to discover where a user's data is hosted.

Key capabilities:

- Public-key based signup and signin.
- Third-party authorization through Pubky auth flows.
- User storage through HTTP `PUT`, `GET`, `DELETE`, and listing APIs.
- PKDNS/Pkarr publishing for homeserver discovery.
- Optional admin and metrics endpoints for operators.

## Repository Layout

| Path | Purpose |
| --- | --- |
| [`pubky-homeserver`](./pubky-homeserver) | Homeserver binary and library crate. |
| [`pubky-sdk`](./pubky-sdk) | Rust SDK for Pubky apps, plus JS/WASM bindings. |
| [`pubky-common`](./pubky-common) | Shared types and helpers used by the SDK and homeserver. |
| [`pubky-testnet`](./pubky-testnet) | Local ephemeral Pubky network for development and tests. |
| [`examples`](./examples) | Rust and JavaScript examples for signup, auth, storage, and requests. |
| [`docs`](./docs) | Source for protocol and concept documentation. |
| [`test_utils`](./test_utils) | Internal testing helpers used by workspace crates. |

## Quick Start

The fastest self-contained check is the Rust testnet example. It starts an ephemeral local network, signs up a user, writes a file, and reads it back. The first run downloads embedded PostgreSQL binaries that are reused on later runs.

```bash
cargo run -p pubky-core-examples --bin testnet
```

For a long-lived local testnet that browser tests, JS examples, and other processes can connect to, run:

```bash
cargo run -p pubky-testnet
```

The long-lived testnet expects PostgreSQL to be available. Use `TEST_PUBKY_CONNECTION_STRING` if your local database does not match the default test connection string.

See the [Rust examples](./examples/rust) and [JavaScript examples](./examples/javascript) for guided flows.

## Run a Homeserver

For a standalone homeserver, install PostgreSQL and create the database expected by the default configuration:

```bash
createdb pubky_homeserver
```

Then start the homeserver:

```bash
cargo run -p pubky-homeserver
```

By default, the homeserver uses `~/.pubky` as its data directory. On first run it creates the data directory, writes `~/.pubky/config.toml`, and creates the homeserver key material.

Default local endpoints:

| Endpoint | Default |
| --- | --- |
| Public HTTP API | `http://127.0.0.1:6286` |
| Pubky TLS API | `127.0.0.1:6287` |
| Admin API | `http://127.0.0.1:6288` |
| Metrics API | `http://127.0.0.1:6289` |

The default signup mode requires signup tokens. See the [homeserver README](./pubky-homeserver/README.md#signup-token) for generating tokens through the admin API.

## Configuration

The homeserver configuration is stored in `config.toml` inside the data directory. The default data directory is `~/.pubky`, and a documented sample is available at [`pubky-homeserver/config.sample.toml`](./pubky-homeserver/config.sample.toml).

Important settings for operators:

- `general.database_url` configures the PostgreSQL connection.
- `general.signup_mode` controls whether signup is open or token-gated.
- `drive.icann_listen_socket` exposes the regular HTTP API.
- `drive.pubky_listen_socket` exposes the Pubky TLS API.
- `storage.type` selects local filesystem, Google Cloud Storage, or in-memory storage.
- `admin.enabled` and `admin.listen_socket` control the admin API.
- `metrics.enabled` and `metrics.listen_socket` control Prometheus metrics.
- `pkdns.public_ip`, `pkdns.icann_domain`, and DHT settings control public discovery.

For production deployments, review the full sample config, isolate admin and metrics endpoints from the public internet, configure a stable PostgreSQL database, and set the public PKDNS values for the machine or reverse proxy that serves the homeserver.

## Use the SDK

Applications normally use the SDK rather than calling homeserver endpoints directly.

Rust:

```toml
[dependencies]
pubky = "0.x"
```

JavaScript and TypeScript:

```bash
npm install @synonymdev/pubky
```

The SDK provides signup, signin, public storage reads, authenticated storage writes, PKDNS resolution, and Pubky auth flows. Start with the [`pubky-sdk` README](./pubky-sdk) or the [examples](./examples).

## Development

Prerequisites:

- Rust `1.89` or newer.
- PostgreSQL for homeserver tests and standalone homeserver runs.
- Node.js `20` or newer for JS/WASM bindings.
- `wasm-pack` when working on the JavaScript SDK bindings.

Useful commands:

```bash
cargo check --workspace --all-features
cargo fmt --check
cargo clippy --workspace --all-features --exclude pubky-wasm -- -D warnings
```

Run Rust tests for a specific crate with a PostgreSQL connection string configured for test databases:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://localhost:5432/postgres?pubky-test=true' cargo test -p pubky-homeserver --all-features
```

Build and test the JavaScript bindings:

```bash
cd pubky-sdk/bindings/js/pkg
npm install
npm run build
npm run testnet
```

Then run the JS tests from another terminal in the same directory:

```bash
npm test
```

## Docker

The Dockerfile is available for isolated local tinkering and keeps the current build behavior.

Build an image:

```bash
docker build --build-arg TARGETARCH=x86_64 -t pubky:core .
```

Run it with log output:

```bash
docker run -it pubky:core
```

Use `--network=host` when the container needs access to host networking or when you want local endpoints to be reachable from the host machine.

## Troubleshooting

`database "pubky_homeserver" does not exist`: create the database or update `general.database_url` in `~/.pubky/config.toml`.

`connection refused` from examples or SDK tests: start `cargo run -p pubky-testnet` first, or make sure your homeserver is running on the configured ports.

`address already in use`: change the relevant listen socket in `~/.pubky/config.toml` or stop the process already using that port.

## Links

- [Pubky website](https://pubky.org/)
- [Documentation](https://docs.pubky.org/)
- [Pkarr](https://pkarr.org/)
- [Contributors Guide](./CONTRIBUTORS.md)
- [Release Process](./RELEASING.md)
