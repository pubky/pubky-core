# Pubky Homeserver

A pubky-core homeserver that acts as users' agent on the Internet, providing data availability and more.more.more.more.

## Usage

Use `cargo run`

```bash
cargo run -- --config=./src/config.toml
```

Or Build first then run from target.

Build

```bash
cargo build --release
```

Run with an optional config file

```bash
../target/release/pubky-homeserver --config=./src/config.toml
```

## Testnet

To run a local homeserver for testing with an internal Pkarr Relay, hardcoded well known publickey and only connected to local Mainline testnet:

```bash
cargo run -- --testnet
```
