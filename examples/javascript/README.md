# Pubky JS Examples (Node 20+)

Tiny CLI scripts that teach the **@synonymdev/pubky** SDK with real flows.

## Prerequisites

- **Node 20+** (fetch + WebCrypto)
- **npm** (or `pnpm`/`yarn`)
- (Optional) A **local testnet** so you can develop fully offline:

  ```bash
  # Requires the rust toolchain
  cargo install pubky-testnet
  pubky-testnet
  ```

  This spins up a local DHT + homeserver + Pkarr relay + Http relay. Scripts that take `--testnet` will use it.

## Install

```bash
npm install
```

> The examples depend on `@synonymdev/pubky`.

## Scripts

Each script is a single, commented file under the project root. Run with `npm run <name> -- <args…>`.

### 1) Testnet End-to-end roundtrip (signup -> write -> read)

Creates a random user, signs up on the local testnet, writes a file to `/pub/my.app/hello.txt`, and reads it back.

```bash
npm run testnet
```

### 2) Signup with a recovery file

Decrypts a recovery file, creates a `Signer`, and signs up on a homeserver.

```bash
npm run signup -- <homeserver_pubky> </path/to/recovery_file> [invitation_code] [--testnet]

# example (testnet homeserver)
npm run signup -- 8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo ./alice.recovery INVITE-123 --testnet
```

You’ll be prompted for the recovery **passphrase**.

### 3) Approve a Pubky Auth URL (authenticator)

Given a `pubkyauth://` URL (QR/deeplink), approves it using a recovery file.
With `--testnet`, it first ensures the user exists by doing a signup (no invite required).

```bash
npm run authenticator -- </path/to/recovery_file> "<AUTH_URL>" [--testnet] [--homeserver <pk>]
```

Example URL looks like:

```
pubkyauth:///?caps=/pub/my.app/:rw&secret=<...>&relay=http://localhost:15412/link
```

You can run a Browser 3rd party app that requires authentication with [**3rd-party-app**](/examples/rust/3-auth_flow/3rd-party-app)

### 4) Public storage read (no auth)

Reads a public resource via the **addressed** form: `<pubky>/pub/my.app/path/to/file.txt`.

```bash
npm run storage -- <pubky>/<absolute-path> [--testnet]

# examples
npm run storage -- q5oo7ma.../pub/my.app/hello.txt --testnet
npm run storage -- operrr8w.../pub/pubky.app/posts/0033X02JAN0SG
```

Shows **exists**, **stats**, and downloads the content.

### 5) Raw HTTP request (pubky:// or https://)

Low-level fetch through the Pubky client. Handy for debugging.

```bash
npm run request -- <METHOD> <URL> [--testnet] [-H "Name: value"]... [-d DATA]

# pubky:// read (testnet)
npm run request -- GET pubky://q5oo7ma.../pub/my.app/info.json --testnet

# https:// JSON POST
npm run request -- \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"msg":"hello"}' \
  POST https://example.com/data.json
```

## Concepts you’ll bump into

- **Pubky** facade: `new Pubky()` (mainnet defaults) or `Pubky.testnet()` (localhost wiring).
- **Signer** -> **Session**: `signer.signin(homeserver, invite?)` -> returns `session`.
- **SessionStorage** (read/write): absolute paths like `"/pub/my.app/file.txt"`.
- **PublicStorage** (read-only): addressed paths like `"<pubky>/pub/my.app/file.txt"`.
- **pubky:// scheme**: `pubky://<pubky>/<abs-path>`, supported by the Pubky client.
- **Recovery file**: encrypted root secret; decrypted with a passphrase to get a `Keypair`.

## Quick troubleshooting

- **ECONNREFUSED / transport errors**
  The local testnet probably isn’t running. Start it:

  ```bash
  pubky-testnet
  ```

- **401 Unauthorized**
  You tried to write without a valid session cookie (e.g., after `signout()`), or against the wrong user.
- **403 Forbidden**
  You tried to write outside `/pub/` (e.g., `/priv/...` is not allowed).
- **Wrong passphrase**
  Decryption of the recovery file fails—double-check the passphrase.
- **Windows paths / quoting**
  Wrap `"<AUTH_URL>"` in quotes, and use proper file paths for recovery files.
