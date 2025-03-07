# Pkarr Republisher

> Early version. Expect breaking API changes. Can still be heavily performance optimized especially by improving the `mainline` lib.

A pkarr packet republisher. Takes [pkarr](https://github.com/pubky/pkarr) and makes it resilient to UDP unreliabilities and CPU exhaustion
by retrying operations with an exponential backoff. Retries help with UDP packet loss and the backoff gives the CPU time to recover.

## Usage

```rust

```
