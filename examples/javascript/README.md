# Pubky JS Examples (Node 20+)

Tiny CLI scripts that teach the **@synonymdev/pubky** SDK with real flows.

> Tip: call `setLogLevel("debug")` when exploring the scripts to surface SDK logs in your console.

## Prerequisites

- **Node 20+** (fetch + WebCrypto)
- **npm**
- **Rust toolchain** for local checkout usage. These examples link to the local JS SDK package under `pubky-sdk/bindings/js/pkg`, so the SDK package must be built before the examples can import it.

## Install

```bash
cd pubky-sdk/bindings/js/pkg
npm install
npm run build

cd ../../../../examples/javascript
npm install
```

> The examples depend on the local `@synonymdev/pubky` package in this repo. If you see a missing `index.cjs` or `pubky_bg.wasm`, run `npm run build` in `pubky-sdk/bindings/js/pkg`. If you see a missing `fetch-cookie`, run `npm install` in that SDK package.

## Local Testnet

Scripts that take `--testnet` expect a local testnet process to be running in another terminal:

```bash
cd pubky-sdk/bindings/js/pkg
npm run testnet
```

Wait for `Testnet running`. This starts a local DHT, homeserver, Pkarr relay, and HTTP relay. Keep that terminal open while running examples.

To check the basic flow from another terminal:

```bash
cd examples/javascript
npm run testnet
```

Expected output includes:

```text
Data write succeeded on path: /pub/my-cool-app/hello.txt
Roundtrip succeeded, response data: hi
```

## Scripts

Each script is a single, commented file under the project root. Run with `npm run <name> -- <args…>`.

### 0) Logging and verbosity

Demonstrates how to use `setLogLevel()` to surface the SDK's internal tracing while performing a quick storage roundtrip.

```bash
npm run logging -- --testnet --level debug
```

Override `--homeserver` when pointing at mainnet infrastructure, or change `--level` to reduce the noise.

### 1) Testnet End-to-end roundtrip (signup -> signin -> write -> read)

Creates a random user, signs up on the local testnet, signs in with a grant-backed session, writes a file to `/pub/my-cool-app/hello.txt`, and reads it back.

```bash
npm run testnet
```

### 2) Signup with a recovery file

Decrypts a recovery file, creates a `Signer`, and signs up on a homeserver.

```bash
npm run signup -- <homeserver_pubky> </path/to/recovery_file> [invitation_code] [--testnet]

# example (testnet homeserver)
npm run signup -- pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo ./alice.recovery INVITE-123 --testnet
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
pubkyauth:///?caps=/pub/my-cool-app/:rw&secret=<...>&relay=http://localhost:15412/inbox
```

You can run a Browser 3rd party app that requires authentication with [**3rd-party-app**](/examples/rust/3-auth_flow/3rd-party-app)

### 4) Public storage read (no auth)

Reads a public resource via the **addressed** form: `pubky<z32>/pub/my-cool-app/path/to/file.txt`.
This requires a public resource whose Pubky key is already resolvable. It is not the best first smoke test for a fresh local testnet user because PKDNS publication can lag or fail independently of authenticated storage.

```bash
npm run storage -- <pubky>/<absolute-path> [--testnet]

# examples
npm run storage -- pubkyq5oo7ma.../pub/my-cool-app/hello.txt --testnet
npm run storage -- pubkyoperrr8w.../pub/pubky.app/posts/0033X02JAN0SG
```

Shows **exists**, **stats**, and downloads the content.

### 5) Raw HTTP request (https://\_pubky.<public_key>)

Low-level fetch through the Pubky client. Handy for debugging.

> Use the **raw z-base32** key (no `pubky` prefix) in the `_pubky.<key>` host portion. Call `publicKey.z32()` to get it.
> As with `storage`, Pubky URLs require a resolvable public key record.

```bash
  npm run request -- <METHOD> <URL> [--testnet] [-H "Name: value"]... [-d DATA]

# pubky:// read (testnet)
npm run request -- GET https://_pubky.q5oo7ma.../pub/my-cool-app/info.json --testnet

# https:// JSON POST
npm run request -- \
  -H "Content-Type: application/json" \
  -H "Accept: application/json" \
  -d '{"msg":"hello"}' \
  POST https://example.com/data.json
```

## Concepts you’ll bump into

- **Pubky** facade: `new Pubky()` (mainnet defaults) or `Pubky.testnet()` (localhost wiring).
- **Signer** -> **Session**: `signer.signup(homeserver, invite?)` creates the account; `signer.signin(clientId)` returns a grant-backed `session`.
- **SessionStorage** (read/write): absolute paths like `"/pub/my-cool-app/file.txt"`.
- **PublicStorage** (read-only): addressed paths like `"<pubky>/pub/my-cool-app/file.txt"`.
- **https://\_pubky.<key> subdomains and key TLDs**: `https://_pubky.<public_key>/<abs-path>`, supported by the Pubky client.
- **Recovery file**: encrypted root secret; decrypted with a passphrase to get a `Keypair`.

## Quick troubleshooting

- **Cannot find `index.cjs` / `pubky_bg.wasm`**
  Build the local SDK package:

  ```bash
  cd pubky-sdk/bindings/js/pkg
  npm run build
  ```

- **Cannot find module `fetch-cookie`**
  Install the local SDK package dependencies:

  ```bash
  cd pubky-sdk/bindings/js/pkg
  npm install
  ```

- **ECONNREFUSED / transport errors**
  The local testnet probably isn’t running, or it is still starting. Start it in another terminal and wait for `Testnet running`:

  ```bash
  cd pubky-sdk/bindings/js/pkg
  npm run testnet
  ```

- **PkarrError: No HTTPS endpoints found**
  The testnet is not running, is not ready, or the public key has not published/resolved yet. Use `npm run testnet` as the first smoke test because it performs an authenticated write/read without requiring public PKDNS resolution for the new user.

- **401 Unauthorized**
  You tried to write without a valid session cookie (e.g., after `signout()`), or against the wrong user.
- **403 Forbidden**
  You tried to write outside `/pub/` (e.g., `/priv/...` is not allowed).
- **Wrong passphrase**
  Decryption of the recovery file fails—double-check the passphrase.
- **Windows paths / quoting**
  Wrap `"<AUTH_URL>"` in quotes, and use proper file paths for recovery files.
