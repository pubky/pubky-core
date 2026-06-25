<h1 align="center"><a href="https://pubky.org/"><img alt="Pubky" src="./.svg/pubky-core-logo.svg" width="200" /></a></h1>

<h3 align="center">
  Reference Homeserver implementation for Pubky.
</h3>

<div align="center">
  <h3>
    <a href="https://docs.pubky.org/">Docs</a>
    <span> | </span>
    <a href="https://docs.rs/pubky">Rust SDK</a>
    <span> | </span>
    <a href="https://www.npmjs.com/package/@synonymdev/pubky">JavaScript SDK</a>
  </h3>
  <a href="https://github.com/pubky/pubky-core/releases/latest/"><img src="https://img.shields.io/github/v/release/pubky/pubky-core" alt="GitHub Release" /></a>
  <a href="https://github.com/pubky/pubky-core/blob/main/LICENSE"><img src="https://img.shields.io/github/license/pubky/pubky-core" alt="GitHub License" /></a>
  <a href="https://crates.io/crates/pubky"><img src="https://img.shields.io/crates/v/pubky" alt="Crates.io Version" /></a>
  <a href="https://www.npmjs.com/package/@synonymdev/pubky"><img src="https://img.shields.io/npm/v/@synonymdev/pubky" alt="npm Version" /></a>
  <a href="https://deepwiki.com/pubky/pubky-core"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki" /></a>
</div>

## What Is This Repository?

This repository contains the reference Pubky homeserver implementation and the crates needed to run, test, and integrate with it - including the Rust and JavaScript SDKs, a local testnet, and examples.

## What Is a Homeserver?

A Pubky Homeserver stores and serves user data. It exposes HTTP APIs for authenticated writes and public reads, and publishes DNS records so other clients can discover where a user's data lives.

- Public-key based signup and signin
- Third-party authorization through Pubky auth flows
- File storage via HTTP `PUT`, `GET`, `DELETE`, and listing APIs (WebDAV-like)
- PKDNS/Pkarr publishing for homeserver discovery
- Admin and metrics endpoints for operators

## Repository Layout

| Path | Purpose |
| --- | --- |
| [`pubky-homeserver`](./pubky-homeserver) | Homeserver binary and library crate. |
| [`pubky-sdk`](./pubky-sdk) | Rust client for Pubky apps, plus JS/WASM bindings. |
| [`pubky-common`](./pubky-common) | Shared types and helpers used by the SDK and homeserver. |
| [`pubky-testnet`](./pubky-testnet) | Local ephemeral Pubky network for development and tests. |
| [`examples`](./examples) | Rust and JavaScript examples for signup, auth, storage, and requests. |
| [`docs`](./docs) | Install guides, local development, and testing docs. |

## Getting Started

| I want to... | Guide |
| --- | --- |
| Run a homeserver | [Install and Run Pubky Homeserver](./docs/INSTALL.md) |
| Develop locally against a testnet | [Local Development](./docs/LOCAL_DEVELOPMENT.md) |
| Run tests and CI | [Testing](./docs/TESTING.md) |

## Use the SDK

The SDK is the easiest way to interact with a homeserver from your app.

Rust:

```toml
[dependencies]
pubky = "0.x"
```

JavaScript and TypeScript:

```bash
npm install @synonymdev/pubky
```

See the [`pubky-sdk` README](./pubky-sdk) for API details or browse the [examples](./examples).

## Development

Prerequisites:

- Rust `1.89` or newer.
- PostgreSQL (see [Local Development](./docs/LOCAL_DEVELOPMENT.md#set-up-postgresql) for setup).
- Node.js `20` or newer for JS/WASM bindings.
- `wasm-pack` when working on the JavaScript SDK bindings.

Useful commands:

```bash
cargo check --workspace --all-features
cargo fmt --check
cargo clippy --workspace --all-features --exclude pubky-wasm -- -D warnings
```

See [Testing](./docs/TESTING.md) for running tests, and [Local Development](./docs/LOCAL_DEVELOPMENT.md) for running a local testnet.

## Links

- [Pubky website](https://pubky.org/)
- [Documentation](https://docs.pubky.org/)
- [Pkarr](https://pkarr.org/)
- [Contributors Guide](./CONTRIBUTORS.md)
- [Release Process](./RELEASING.md)

---

May the power ⚡ be with you. Powered by [pkarr](https://github.com/pubky/pkarr).