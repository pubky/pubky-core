# Pubky SDK (Rust)

Ergonomic building blocks for Pubky apps: a tiny HTTP/PKARR client, session-bound agent, a drive API, signer helpers, and a pairing-based auth flow for keyless apps.

Rust implementation of [Pubky](https://github.com/pubky/pubky-core) SDK.

## Install

```toml
# Cargo.toml
[dependencies]
pubky = "0.x"            # this crate
# Optional helpers used in examples:
# pubky-testnet = "0.x"
```

## Quick start

```rust ignore
use pubky::prelude::*; // pulls in the common types

# async fn run() -> pubky::Result<()> {

// 1) Create a new random key user bound to a Signer
let signer = PubkySigner::new(Keypair::random())?;

// 1) Sign up on a homeserver (identified by its public key)
let homeserver = PublicKey::try_from("o4dksf...uyy")?;
let agent = signer.signup_agent(&homeserver, None).await?;

// 2) Session-scoped drive I/O
agent.drive().put("/pub/app/hello.txt", "hello").await?;
let body = agent.drive().get("/pub/app/hello.txt").await?.bytes().await?;
assert_eq!(&body, b"hello");

// 3) Public (unauthenticated) read by user-qualified path
let txt = PubkyDrive::public()?
  .get(format!("{}/pub/app/hello.txt", agent.public_key()))
  .await?
  .text().await?;
assert_eq!(txt, "hello");

// 4) Publish / resolve your PKDNS (_pubky) record
signer.pkdns().publish_homeserver_if_stale(None).await?;
let resolved = Pkdns::new()?.get_homeserver(&signer.public_key()).await;
println!("current homeserver: {:?}", resolved);

// 5) Keyless third-party app: pairing auth → agent
let caps = Capabilities::builder().write("/pub/pubky.app/").finish();
let (sub, url) = PubkyPairingAuth::new(&caps)?.subscribe();
// show `url` (QR/deeplink); on the signing device call:
// signer.approve_pubkyauth_request(&url).await?;
let app_agent = sub.wait_for_approval().await?;

# Ok(()) }
```

## Concepts at a glance

Transport:

- **`PubkyHttpClient`** stateless transport: PKARR-aware HTTP with sane defaults.

High level actors:

- **`PubkySigner`** high-level signer (keypair holder) with `signup`, `signin`, publishing, and pairing auth approval.
- **`PubkyAgent`** session-bound identity (holds a `Session` & cookie). Use `agent.drive()` for reads/writes.
- **`PubkyPairingAuth`** pairing auth flow for keyless apps via an HTTP relay.
- **`PubkyDrive`** simple file-like API: `get/put/post/patch/delete`, plus `exists()`, `stats()` and `list()`.
- **`Pkdns`** resolve/publish `_pubky` Pkarr records (read-only via `Pkdns::new()`, publishing when created from a `PubkySigner`).

## Pairing auth (keyless apps)

```rust ignore
# use pubky::prelude::*;
# async fn pairing() -> pubky::Result<()> {
let caps = Capabilities::builder().rw("/pub/acme.app/").finish();

// Easiest: use the default relay (see “Relay” notes below)
let (sub, url) = PubkyPairingAuth::new(&caps)?.subscribe();
// show `url` to the user (QR or deeplink). On the signer device:
/// signer.approve_pubkyauth_request(&url).await?;

let agent = sub.wait_for_approval().await?; // background long-polling started by `subscribe`
# Ok(()) }
```

### Relay & reliability

- If you don’t pass a relay, we default to a Synonym-hosted instance. If that relay is down, logins won’t complete.
- For production and bigger apps, run your **own relay** (MIT, dockerable): [https://httprelay.io](https://httprelay.io)
  Derive the channel as `base64url(hash(secret))`; the token is end-to-end encrypted with the `secret`.

## Drive API (session & public)

```rust ignore
# use pubky::prelude::*;
# async fn io(agent: &PubkyAgent) -> pubky::Result<()> {
// write
agent.drive().put("/pub/app/file.txt", "hi").await?;

// read raw
let bytes = agent.drive().get("/pub/app/file.txt").await?.bytes().await?;

// metadata / existence
let meta = agent.drive().stats("/pub/app/file.txt").await?;
let ok = agent.drive().exists("/pub/app/missing.txt").await?; // false

// public read by user-qualified path (no session)
let public = PubkyDrive::public()?;
let text = public.get(format!("{}/pub/app/file.txt", agent.public_key()))
    .await?.text().await?;
# Ok(()) }
```

## PKDNS (`_pubky`) Pkarr publishing

```rust ignore
# use pubky::prelude::*;
# async fn pkdns(signer: &PubkySigner) -> pubky::Result<()> {
// Republish only if stale (recommended in app start)
signer.pkdns().publish_homeserver_if_stale(None).await?;

// Force a homeserver record publish (e.g., migration)
signer.pkdns().publish_homeserver_force("homeserver_pubky").await?;
# Ok(()) }
```

## Paths & addressing

Use absolute paths for agent-scoped I/O (`"/pub/…"`), or user-qualified forms when public:

```rust ignore
# use pubky::prelude::*;
# fn addr_examples(user: &PublicKey) -> pubky::Result<()> {
let a = PubkyPath::new(Some(user.clone()), "/pub/app/file.txt")?;
let b: PubkyPath = (user.clone(), "/pub/app/file.txt").into_pubky_path()?;
# Ok(()) }
```

## Features

- `json` enable `drive::json` helpers (`get_json` / `put_json`) and serde on certain types.

## Testing locally

Spin up an ephemeral testnet (DHT + homeserver + relay) and run your tests fully offline:

```
# use pubky_testnet::{EphemeralTestnet, pubky::prelude::*};
# async fn test() -> pubky_testnet::pubky::Result<()> {

let testnet = EphemeralTestnet::start().await.unwrap();
let homeserver  = testnet.homeserver();

let signer = PubkySigner::random()?;
let agent  = signer.signup(&homeserver.public_key(), None).await?;

agent.drive().put("/pub/app/hello.txt", "hi").await?;
let s = agent.drive().get("/pub/app/hello.txt").await?.text().await?;
assert_eq!(s, "hi");

# Ok(()) }
```

## Session persistence (scripts that restart)

Export a compact bearer token and import it later to avoid re-auth:

```rust no_run
# use pubky::prelude::*;
# async fn persist(agent: &PubkyAgent, client: &PubkyHttpClient) -> pubky::Result<()> {
// Save
let secret = agent.export_secret();               // "<pubkey>:<cookie_secret>"
// store `token` securely (env, keychain, vault). DO NOT log it.

// Restore
let restored = PubkyAgent::import_secret(client, &secret).await?;
// Optional sanity check:
restored.revalidate_session().await?;
# Ok(()) }
```

> Security: the cookie secret is a **bearer token**. Anyone holding it can act as the user within the granted capabilities. Treat it like a password.

## Design notes

- **Blocking vs managed pairing:** prefer `subscribe()/wait_for_approval()` (starts polling immediately when you get the URL) to avoid missing approvals. If you manually fetch the URL before polling, you can race the signer and miss the one-shot response.
- **Stateless client, stateful agent:** `PubkyHttpClient` never holds identity; `PubkyAgent` does.

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) using the Pubky SDK.

## JS bindings

Find a wrapper of this crate using `wasm_bindgen` in `pubky-sdk/bindings/js`

---

**License:** MIT
**Relay:** [https://httprelay.io](https://httprelay.io) (open source; run your own for production)
