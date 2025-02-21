# Pubky Homeserver

A pubky-core homeserver that acts as users' agent on the Internet, providing data availability and more.

## Usage

### Library

You can use the Homeserver as a library in other crates/binaries or for testing purposes.

```rust
use anyhow::Result;
use pubky_homeserver::Homeserver;

#[tokio::main]
async fn main() {
    let homeserver = unsafe {
        Homeserver::builder().run().await.unwrap()
    };

    println!("Shutting down Homeserver");

    homeserver.shutdown();
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
