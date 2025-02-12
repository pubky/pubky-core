# HTTP Relay

A Rust implementation of _some_ of [Http relay spec](https://httprelay.io/).

Normally you are better off running the [reference implementation's binary](https://httprelay.io/download/).

This implementation, for the time being is meant for having a convenient library to be used in unit tests, and testnets in Pubky.

## Usage

```rust
#[tokio::main]
async fn main() {
    let http_relay = http_relay::HttpRelay::builder()
        .http_port(15412)
        .build()
        .await
        .unwrap();

    println!(
        "Running http relay {}",
        http_relay.local_link_url().as_str()
    );

    tokio::signal::ctrl_c().await.unwrap();

    http_relay.shutdown();
}
```
