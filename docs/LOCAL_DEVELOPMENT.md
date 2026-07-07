# Local Development

This guide is for app developers and contributors who need a local Pubky network to connect browsers, SDK examples, and integration tests to. For a standalone homeserver deployment, see [Install and Run Pubky Homeserver](./INSTALL.md).

## Run the Local Testnet

The easiest local setup is the long-lived testnet with embedded PostgreSQL:

```bash
cargo run -p pubky-testnet --features embedded-postgres -- --embedded-postgres
```

It starts:

| Component | Default |
| --- | --- |
| DHT bootstrap node | `127.0.0.1:6881` |
| Pkarr relay | `http://127.0.0.1:15411` |
| HTTP relay | `http://127.0.0.1:15412` |
| Homeserver HTTP API | `http://127.0.0.1:6286` |
| Homeserver Pubky TLS API | `127.0.0.1:6287` |
| Homeserver admin API | `http://127.0.0.1:6288` |

The static testnet homeserver uses this public key:

```text
pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo
```

This mode is intended for local development only. It uses test configuration, local ports, and disposable test data.

## Embedded PostgreSQL

The first run downloads PostgreSQL binaries and caches them for later runs. The embedded database is stopped when the testnet process exits.

Use this mode when you want the fewest local dependencies and do not need to inspect a persistent database.

## External PostgreSQL

If you want to use your own PostgreSQL instance, omit `--embedded-postgres` and set `TEST_PUBKY_CONNECTION_STRING` when needed:

```bash
TEST_PUBKY_CONNECTION_STRING='postgres://postgres:postgres@localhost:5432/postgres?pubky-test=true' \
  cargo run -p pubky-testnet
```

The `?pubky-test=true` parameter tells the homeserver test configuration to create an ephemeral `pubky_test_*` database inside the configured PostgreSQL instance.

For a local Docker PostgreSQL instance:

```bash
docker run --name pubky-postgres \
  -e POSTGRES_USER=postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=postgres \
  -p 127.0.0.1:5432:5432 \
  -d postgres:18-alpine
```

## Run Examples Against the Testnet

With the long-lived testnet running, use the examples in a separate terminal.

Rust examples are in [`examples/rust`](../examples/rust):

```bash
cargo run -p pubky-core-examples --bin signup -- --testnet pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo /tmp/pubky-recovery.json
```

JavaScript examples are in [`examples/javascript`](../examples/javascript). They use the local testnet defaults when configured for testnet mode.

For a quick self-contained SDK smoke test, run:

```bash
cargo run -p pubky-core-examples --bin testnet
```

That example starts its own ephemeral testnet, signs up a user, writes a file, and reads it back.

## Custom Homeserver Config

The testnet binary accepts a homeserver config path:

```bash
cargo run -p pubky-testnet -- --homeserver-config ./path/to/config.toml
```

Use this when you need to test a specific homeserver configuration while still using the local DHT and relay setup.

## Troubleshooting

### Examples Cannot Connect

Make sure the long-lived testnet is still running and that the expected ports are not blocked or already used by another process.

### Port Already In Use

Stop the process using the conflicting port or use a custom homeserver config for the homeserver ports. The static DHT and relay ports are fixed for the long-lived testnet.

### Embedded PostgreSQL Download Fails

The embedded PostgreSQL binary is downloaded from GitHub releases. If you hit API rate limits, set a GitHub token and retry:

```bash
export GITHUB_TOKEN=ghp_your_personal_access_token
cargo run -p pubky-testnet --features embedded-postgres -- --embedded-postgres
```

The token does not need repository permissions.

### Reset Embedded PostgreSQL Cache

If the cached PostgreSQL download appears corrupt, remove the cache and rerun the command:

```bash
rm -rf ~/.cache/pubky-testnet/postgresql
```
