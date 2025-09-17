# Pkarr Republisher

> Early version. Expect breaking API changes. Can still be heavily performance optimized especially by improving the `mainline` lib.

To keep data on the Mainline DHT alive it needs to be actively republished every hour. This library provides the tools to republish packets reliably and in a multi-threaded fashion. This allows the homeserver to republish hundreds of thousands of pkarr keys per day.



## Usage

**ResilientClient** Pkarr Client with retry and exponential backoff.

Takes [pkarr](https://github.com/pubky/pkarr) and makes it resilient to UDP unreliabilities and CPU exhaustion
by retrying operations with an exponential backoff. Retries help with UDP packet loss and the backoff gives the CPU time to recover.

```rust
use pkarr_republisher::ResilientClient;

let client = ResilientClient::new().unwrap();
let public_key = Keypair::random().public_key();

// Republish with retries
match client.republish(public_key.clone(), None).await {
    Ok(info) => {
        println!("Key {public_key} published to {} nodes after {} attempt(s).", info.published_nodes_count, info.attempts_needed);    
    },
    Err(err) => {
        if err.is_missing() {
            println!("Key {public_key} not found in DHT.");
        }
        if err.is_publish_failed() {
            println!("Key {public_key} failed to publish. {err}");
        }
    }
}
```

> **Limitation** `ResilientClient` requires a pkarr client that was built with the `dht` feature.
> Relays only are not supported.

**MultiRepublisher** Multi-threaded republisher of pkarr keys.

Uses the `ResilientClient` to publish hundreds of thousands of pkarr keys per day.

```rust
use pkarr_republisher::MultiRepublisher;
use pkarr::{Keypair, PublicKey};

let public_keys: Vec<PublicKey> = (0..100).map(|_| Keypair::random().public_key()).collect();
let republisher = MultiRepublisher::new().unwrap();
let results = republisher.run(public_keys, 10).await.expect("UDP socket build infallible");

// Verify result of each republished key.
for (key, result) in results {
    match result {
        Ok(info) => {
            println!("Key {} published to {} nodes after {} attempt(s).", key, info.published_nodes_count, info.attempts_needed);
        },
        Err(err) => {
            if err.is_missing() {
                println!("Key {} not found in DHT.", key);
            } else if err.is_publish_failed() {
                println!("Key {} failed to publish: {}", key, err);
            } else {
                println!("Key {} encountered an error: {}", key, err);
            }
        }
    }
}
```

> **Limitation** Publishing a high number of pkarr keys is CPU intense. A recent test showed a 4 Core CPU being able to publish ~600,000 keys in 24hrs.
> Takes this into consideration.
> Do not use pkarr relays with the `MultiRepublisher`. You will run into rate limits which are currently not handled.


## Examples

The [examples folder](./examples) contains scripts to test the performance of the republisher.

- [publish_and_save](./examples/publish_and_save.rs) Publishes x keys multi-threaded and saves them in `published_secrets.txt`.
- [read_and_verify](./examples/read_and_verify.rs) Takes a random sample of the published keys and verifies on how many nodes they've been stored on.
- [read_and_republish](./examples/read_and_republish.rs) takes the saved keys and republishes them multi-threaded.

Execute with `cargo run --example publish_and_save`