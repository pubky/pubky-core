# Storage

Read and write data on Pubky homeservers.

## Public Read (unauthenticated)

Get public data from any user's homeserver storage — no keys needed.

```bash
cargo run --bin storage read pubky<user>/pub/<path>

# example
cargo run --bin storage read pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG

# testnet mode
cargo run --bin storage read -- --testnet pubky<user>/pub/<path>
```

## Authenticated Write

Write, read back, and delete a file on your own homeserver.

### Recovery File

This example defaults to `../sample_recovery.key`, which has an empty passphrase.
If that sample key cannot be decrypted with an empty passphrase, the CLI prompts for a passphrase.

Sign up to a homeserver first using the [signup example](../2-signup).

```bash
cargo run --bin storage write

# custom path and content
cargo run --bin storage write --content "my data" /pub/my-app/data.json

# with a custom recovery file
cargo run --bin storage write </path/to/recovery file>
```

You can pass `--testnet` to use the local testnet defaults.
