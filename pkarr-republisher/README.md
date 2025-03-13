# Pkarr Republisher

> Early version. Expect breaking API changes. Can still be heavily performance optimized especially by improving the `mainline` lib.

A pkarr packet republisher. Takes [pkarr](https://github.com/pubky/pkarr) and makes it resilient to UDP unreliabilities and CPU exhaustion
by retrying operations with an exponential backoff. Retries help with UDP packet loss and the backoff gives the CPU time to recover.

## Usage

**ResilientClient** Pkarr Client with retry and exponential backoff.

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

**MultiRepublisher** Multi-threaded republisher of pkarr keys.

```rust
use pkarr_republisher::MultiRepublisher;
use pkarr::{Keypair, PublicKey};

let public_keys: Vec<PublicKey> = (0..100).map(|_| Keypair::random().public_key()).collect();
let republisher = MultiRepublisher::new().unwrap();
let results = republisher.run(public_keys, 10).await.expect("UDP socket build infalliable");

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



## Examples

The [src/bin folder](./src/bin) contains example scripts to test the performance of the republisher.

- [publish_and_save](./src/bin/publish_and_save.rs) Publishes x keys multi-threaded and saves them in `published_secrets.txt`.
- [read_and_verify](./src/bin/read_and_verify.rs) Takes a random sample of the published keys and verifies on how many nodes they've been stored on.
- [read_and_republish](./src/bin/read_and_republish.rs) takes the saved keys and republishes them multi-threaded.