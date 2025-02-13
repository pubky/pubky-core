# Pubky Homeserver

A pubky-core homeserver that acts as users' agent on the Internet, providing data availability and more.

## Usage

### Library

You can use the Homeserver as a library in other crates/binaries or for testing purposes.

```rust

#[tokio::main]
async fn main() -> Result<()> {
    Homeserver::builder().run().await?

    tokio::signal::ctrl_c().await?;

    tracing::info!("Shutting down Homeserver");

    server.shutdown();

    Ok(())
}
```

### Binary

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
