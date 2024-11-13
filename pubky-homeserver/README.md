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
../target/release/pubky_homeserver --config=./src/config.toml
```

## Testnet

Testnet is a mode where the Homeserver is running locally, connected to an internal Mainline Testnet (not the public DHT), and acting as a Pkarr relay for clients in web browsers.

You can run a homeserver in Testnet by passing an argument:

```bash
cargo run --testnet
```

Or set the `testnet` field in the passed config file to true.
