# Storage

Get public data from homeserver storage of a user.

## Usage

Get data from a Pubky user homeserver storage using an addressed resource identifier.

```bash
cargo run --bin storage pubky<user>/pub/<path>
```

For example, at the time of writing, the following command returns the content of a user's social post.

```bash
cargo run --bin storage pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG
```

### Testnet

You can pass a `--testnet` argument to run the query in testnet mode (using local DHT testnet).
