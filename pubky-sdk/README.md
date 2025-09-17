# Pubky SDK (Rust)

Ergonomic building blocks for Pubky apps: a tiny HTTP/PKARR client, session-bound agent, a storage API, signer helpers, and a pairing-based auth flow for keyless apps.

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

```rust no_run
use pubky::prelude::*; // pulls in the common types

# async fn run() -> pubky::Result<()> {

// 1) Create a new random key user bound to a Signer
let signer = PubkySigner::new(Keypair::random())?;

// 2) Sign up on a homeserver (identified by its public key)
let homeserver = PublicKey::try_from("o4dksf...uyy").unwrap();
let session = signer.signup(&homeserver, None).await?;

// 3) Session-scoped storage I/O
session.storage().put("/pub/my.app/hello.txt", "hello").await?;
let body = session.storage().get("/pub/my.app/hello.txt").await?.text().await?;
assert_eq!(&body, "hello");

// 4) Public (unauthenticated) read by user-qualified path
let txt = PubkyStorage::new_public()?
  .get(format!("{}/pub/my.app/hello.txt", session.public_key()))
  .await?
  .text().await?;
assert_eq!(txt, "hello");

// 5) Publish / resolve your PKDNS (_pubky) record
signer.pkdns()?.publish_homeserver_if_stale(None).await?;
let resolved = Pkdns::new()?.get_homeserver(&signer.public_key()).await;
println!("current homeserver: {:?}", resolved);

// 6) Keyless third-party app: pairing auth -> session
let caps = Capabilities::builder().write("/pub/pubky.app/").finish();
let (sub, url) = PubkyAuthRequest::new(&caps)?.subscribe();
// show `url` (QR/deeplink); on the signing device call:
// signer.approve_auth_request(&url).await?;
let app_session = sub.wait_for_approval().await?;

# Ok(()) }
```

## Concepts at a glance

Transport:

- **`PubkyHttpClient`** stateless transport: PKARR-aware HTTP with sane defaults.

High level actors:

- **`PubkySigner`** high-level signer (keypair holder) with `signup`, `signin`, publishing, and pairing auth approval.
- **`PubkySession`** session-bound identity (holds a `SessionInfo` & cookie). Use `session.storage()` for reads/writes.
- **`PubkyAuthRequest`** pairing auth flow for keyless apps via an HTTP relay.
- **`PubkyStorage`** simple file-like API: `get/put/delete`, plus `exists()`, `stats()` and `list()`.
- **`Pkdns`** resolve/publish `_pubky` Pkarr records (read-only via `Pkdns::new()`, publishing when created from a `PubkySigner`).

## Examples

### Storage API (session & public)

Use a `PubkyStorage` to access and write data into Pubky Homeservers.

## Public

Use to access public data from any user without need to authenticate.

```rust no_run
# use pubky::prelude::*;
# async fn run(user: PublicKey) -> pubky::Result<()> {
let storage = PubkyStorage::new_public()?;
// read someone’s public file
let text = storage.get(format!("{}/pub/my.app/data.txt", user)).await?.text().await?;
// writes are not allowed in public mode!
# Ok(()) }
```

[Public Storage example](https://github.com/pubky/pubky-core/tree/main/examples/storage)

## Session

Use to write data into your own homeserver.

```rust no_run
# use pubky::prelude::*;
# async fn run(session: &PubkySession) -> pubky::Result<()> {
let storage = session.storage(); // Equivalent to PubkyStorage::new_from_session(session);
// read from your own homeserver (no need to specify user id)
let text = storage.get("/pub/my.app/data.txt").await?.text().await?;
// writing to your own homeserver
storage.put("/pub/my.app/data.txt", "hi").await?;
# Ok(()) }
```

| Property                    | Session mode (`PubkyStorage::new_from_session()`)             | Public mode (`PubkyStorage::new_public()`)            |
| --------------------------- | ------------------------------------------------------------- | ----------------------------------------------------- |
| Auth                        | Yes (session cookie auto-attached for this user’s homeserver) | No                                                    |
| Path form                   | Absolute, session-scoped (e.g. `/pub/my.app/file.txt`)        | User-qualified (e.g. `<user>/pub/my.app/file.txt`)    |
| Reads                       | Yes                                                           | Yes (public data only)                                |
| Writes                      | Yes                                                           | No (server will reject with 401/403)                  |
| Cookie leakage across users | Never                                                         | N/A                                                   |
| Typical use                 | Your app acting “as the user”                                 | Fetch someone else’s public content without a session |

### Resources & addressing

Use absolute paths for session-scoped I/O (`"/pub/…"`), or user-qualified forms when public:

```rust no_run
# use pubky::prelude::*;
# fn addr_examples(user_pubky: PublicKey) -> pubky::Result<()> {
let a = PubkyResource::new(Some(user_pubky), "/pub/my.app/file.txt")?;
let b: PubkyResource = "{user_public_key}/pub/my.app/file.txt".into_pubky_resource()?;
# Ok(()) }
```

**By convention, prefer to place your app data in a new public `/pub` top level folder with domain-like name. Example `/pub/mycoolnew.app/`**

### PKDNS (`_pubky`) Pkarr publishing

Publish and retrieve pkarr record.

```rust no_run
# use pubky::prelude::*;
# async fn pkdns(signer: &PubkySigner) -> pubky::Result<()> {
// Republish only if stale (recommended in app start)
signer.pkdns()?.publish_homeserver_if_stale(None).await?;

// Force a homeserver record publish (e.g., migration)
let homeserver = PublicKey::try_from("homeserver_pubky").unwrap();
signer.pkdns()?.publish_homeserver_force(Some(&homeserver)).await?;
# Ok(()) }
```

### Pubky QR auth for third-party and keyless apps.

Request auth url and await approval.

```rust
# use pubky::prelude::*;
# async fn auth() -> pubky::Result<()> {
// Read/Write capabilities for acme.app route
let caps = Capabilities::builder().rw("/pub/acme.app/").finish();

// Easiest: uses the default relay (see “Relay” notes below)
let (sub, url) = PubkyAuthRequest::new(&caps)?.subscribe();
// This is the moment to send `url` to the user (QR or deeplink). On the signer device
// for example Pubky Ring application on mobile. The signer will perform this call:
// signer.approve_auth_request(&url).await?;
# PubkySigner::random()?.approve_auth_request(&url).await?;

let session = sub.wait_for_approval().await?; // background long-polling started by `subscribe`
# Ok(()) }
```

See this fully working [Auth Flow example](https://github.com/pubky/pubky-core/tree/main/examples/auth_flow)

#### Relay & reliability

- If you don’t pass a relay, [`PubkyAuthRequest`] defaults to a Synonym-hosted instance. If that relay is down, logins won’t complete.
- For production and bigger apps, run your **own relay** (MIT, dockerable): [https://httprelay.io](https://httprelay.io)
  Derives the channel as `base64url(hash(secret))`; the token is end-to-end encrypted with the `secret`. See `PubkyAuthRequest::new_with_relay` docs for further info.

## Features

- `json` enable `storage::json` helpers (`get_json` / `put_json`) and serde on certain types.

## Testing locally

Spin up an ephemeral testnet (DHT + homeserver + relay) and run your tests fully offline:

```rust
# use pubky_testnet::{EphemeralTestnet, pubky::prelude::*};
# async fn test() -> pubky_testnet::pubky::Result<()> {

let testnet = EphemeralTestnet::start().await.unwrap();
let homeserver  = testnet.homeserver();

let signer = PubkySigner::random()?;
let session  = signer.signup(&homeserver.public_key(), None).await?;

session.storage().put("/pub/my.app/hello.txt", "hi").await?;
let s = session.storage().get("/pub/my.app/hello.txt").await?.text().await?;
assert_eq!(s, "hi");

# Ok(()) }
```

## Session persistence (scripts that restart)

Export a compact bearer token and import it later to avoid re-auth:

```rust no_run
# use pubky::prelude::*;
# async fn persist(session: &PubkySession, client: &PubkyHttpClient) -> pubky::Result<()> {
// Save
let token = session.export_secret();               // "<pubkey>:<cookie_secret>"
// store `token` securely (env, keychain, vault). DO NOT log it.

// Restore
let restored = PubkySession::import_secret(client, &token).await?;
// Optional sanity check:
restored.revalidate().await?;
# Ok(()) }
```

> Security: the cookie secret is a **bearer token**. Anyone holding it can act as the user within the granted capabilities. Treat it like a password.

## Design notes

- **Blocking vs managed pairing:** prefer `subscribe()/wait_for_approval()` (starts polling immediately when you get the URL) to avoid missing approvals. If you manually fetch the URL before polling, you can race the signer and miss the one-shot response.
- **Stateless client, stateful session:** `PubkyHttpClient` never holds identity; `PubkySession` does.

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) using the Pubky SDK.

## JS bindings

Find a wrapper of this crate using `wasm_bindgen` in `pubky-sdk/bindings/js`.

---

**License:** MIT
**Relay:** [https://httprelay.io](https://httprelay.io) (open source; run your own for production)
