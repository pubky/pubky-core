# Changelog

All notable changes to the `pubky-testnet` crate will be documented in this file.

## [0.7.3] - 2026-03-03

### Added

- Embedded Postgres support via `embedded-postgres` feature flag, allowing tests to run without an external Postgres instance
- Unique data directories per embedded Postgres instance to prevent conflicts in parallel test runs

### Changed

- Set explicit versions for all workspace dependencies in `Cargo.toml` for better reproducibility when publishing

### Fixed

- README instructions for running local cargo tests
- macOS test compatibility

## [0.6.0] - 2026-01-13

### Features

- **Builder pattern** for `EphemeralTestnet` configuration, enabling custom keypairs, configs, and HTTP relay settings
- **Random keypair generation** option for ephemeral testnets
- **Configurable relay host** for Docker environments
- **Static testnet configuration** support
- Postgres database backend support (replacing LMDB)
- Flexible files backend (Google Bucket, local filesystem, in-memory)
- Optional admin server
- Docker support with configurable ports
