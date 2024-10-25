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
