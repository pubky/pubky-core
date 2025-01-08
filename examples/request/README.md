# Request

Make HTTP requests over for Pubky authority URL.

## Usage 

Request data from a Pubky's data storage.

```bash
cargo run --bin request GET pubky://<user pubky>/pub/<path>
```

Or make a direct HTTP request.

```bash
cargo run --bin request GET https://<Pkarr domain>/[path]
```

### Testnet

You can pass a `--testnet` argument to run the query in testnet mode (using local DHT testnet).
