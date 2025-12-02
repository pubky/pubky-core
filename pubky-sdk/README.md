# Pubky SDK

Ergonomic building blocks for Pubky apps: one facade (`Pubky`) plus focused actors for sessions, storage API, signer helpers, and QR auth flow for keyless apps.

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
use pubky::prelude::*;

# async fn run() -> pubky::Result<()> {

let pubky = Pubky::new()?; // or Pubky::testnet() for local testnet.

// 1) Create a new random key user and bound to a Signer
let keypair = Keypair::random();
let signer = pubky.signer(keypair);

// 2) Sign up on a homeserver (identified by its public key)
let homeserver = PublicKey::try_from("o4dksf...uyy").unwrap();
let session = signer.signup(&homeserver, None).await?;

// 3) Read/Write as the signed-in user
session.storage().put("/pub/my-cool-app/hello.txt", "hello").await?;
let body = session.storage().get("/pub/my-cool-app/hello.txt").await?.text().await?;
assert_eq!(&body, "hello");

// 4) Public read of another user’s file
let txt = pubky.public_storage()
  .get(format!("pubky{}/pub/my-cool-app/hello.txt", session.info().public_key()))
  .await?
  .text().await?;
assert_eq!(txt, "hello");

// 5) Keyless app flow (QR/deeplink)
let caps = Capabilities::builder().write("/pub/example.com/").finish();
let flow = pubky.start_auth_flow(&caps, AuthFlowKind::signin())?;
println!("Scan to sign in: {}", flow.authorization_url());
let app_session = flow.await_approval().await?;

// 6) Optional (advanced): publish or resolve PKDNS (_pubky) records
signer.pkdns().publish_homeserver_if_stale(None).await?;
let resolved = signer.pkdns().get_homeserver().await;
println!("Your current homeserver: {:?}", resolved);

# Ok(()) }
```

## Mental model

- `Pubky` - facade, always start here! Owns the transport and constructs actors.
- `PubkySigner` - local key holder. Can `signup`, `signin`, approve QR auth, publish PKDNS.
- `PubkySession` - authenticated “as me” handle. Exposes session-scoped storage.
- `PublicStorage` - unauthenticated reads of others’ public data.
- `Pkdns` - resolve/publish `_pubky` records.

#### Transport:

- **`PubkyHttpClient`** : handles requests to pubky public-key hosts.

## Examples

### Storage API (session & public)

Session (authenticated):

```rust no_run
use pubky::{Pubky, Keypair};

# async fn run(keypair: Keypair) -> pubky::Result<()> {

let pubky = Pubky::new()?;
let session = pubky.signer(keypair).signin().await?;

let storage = session.storage();
storage.put("/pub/my-cool-app/data.txt", "hi").await?;
let text = storage.get("/pub/my-cool-app/data.txt").await?.text().await?;

# Ok(()) }
```

Public (read-only):

```rust no_run
use pubky::{Pubky, PublicKey};

# async fn run(user_id: PublicKey) -> pubky::Result<()> {

let pubky = Pubky::new()?;
let public = pubky.public_storage();

let file = public
    .get(format!("pubky{user_id}/pub/example.com/file.bin"))
    .await?
    .bytes()
    .await?;

let entries = public
    .list(format!("pubky{user_id}/pub/example.com/"))?
    .limit(10)
    .send()
    .await?;
for entry in entries {
    println!("{}", entry.to_pubky_url());
}

# Ok(()) }
```

See the [Public Storage example](https://github.com/pubky/pubky-core/tree/main/examples/rust/4-storage).

Path rules:

- Session storage uses **absolute** paths like `"/pub/app/file.txt"`.
- Public storage uses **addressed** form `pubky<user>/pub/app/file.txt` (preferred) or `pubky://<user>/...`.

**Convention:** put your app’s public data under a domain-like folder in `/pub`, e.g. `/pub/my-new-app/`.

### Resolve identifiers into transport URLs

Need to feed a public resource into a raw HTTP client? Use [`resolve_pubky`] to transform the human-facing identifier into the HTTPS homeserver URL:

```rust
# use pubky::resolve_pubky;
# fn main() -> pubky::Result<()> {
let url = resolve_pubky("pubkyoperrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG")?;
assert_eq!(
    url.as_str(),
    "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG"
);
# Ok(())
# }
```

## PKDNS (Pkarr)

Resolve another user’s homeserver (`_pubky` record), or publish your own via the signer.

```rust no_run
use pubky::{Pubky, PublicKey, Keypair};
# async fn run(other: PublicKey, new_homeserver_id: PublicKey) -> pubky::Result<()> {
let pubky = Pubky::new()?;

// read-only homeserver resolver
let host = pubky.get_homeserver_of(&other).await;

// publish with your key
let signer = pubky.signer(Keypair::random());
signer.pkdns().publish_homeserver_if_stale(None).await?;
// or force republish (e.g. homeserver migration)
signer.pkdns().publish_homeserver_force(Some(&new_homeserver_id)).await?;
// resolve your own homeserver
signer.pkdns().get_homeserver().await?;

# Ok(()) }
```

### Pubky QR auth for third-party and keyless apps

Request an authorization URL and await approval.

**Typical usage:**

1. Start an auth flow with `pubky.start_auth_flow(&caps)` (or use the `PubkyAuthFlow::builder()` to set a custom relay).
2. Show `authorization_url()` (QR/deeplink) to the signing device (e.g., [Pubky Ring](https://github.com/pubky/pubky-ring) — [iOS](https://apps.apple.com/om/app/pubky-ring/id6739356756) / [Android](https://play.google.com/store/apps/details?id=to.pubky.ring)).
3. Await `await_approval()` to obtain a session-bound `PubkySession`.

```rust
# use pubky::{Pubky, Capabilities, Keypair, AuthFlowKind};
# async fn auth() -> pubky::Result<()> {

let pubky = Pubky::new()?;
// Read/Write capabilities for acme.app route
let caps = Capabilities::builder().read_write("pub/example.com/").finish();

// Start the flow using the default relay (see “Relay & reliability” below)
let flow = pubky.start_auth_flow(&caps, AuthFlowKind::signin())?;
println!("Scan to sign in: {}", flow.authorization_url());

// On the signing device, approve with: signer.approve_auth(flow.authorization_url()).await?;
# pubky.signer(Keypair::random()).approve_auth(flow.authorization_url()).await?;

let session = flow.await_approval().await?;

# Ok(()) }
```

Approve an auth request

```rust ignore
signer.approve_auth(authorization_url).await?;
```

See the fully functional [**Auth Flow Example**](https://github.com/pubky/pubky-core/tree/main/examples/rust/3-auth_flow).

#### Relay & reliability

- If you don’t specify a relay, `PubkyAuthFlow` defaults to a Synonym-hosted relay. If that relay is down, logins won’t complete.
- For production and larger apps, run **your own relay** (MIT, Docker): [https://httprelay.io](https://httprelay.io).
  The channel is derived as `base64url(hash(secret))`; the token is end-to-end encrypted with the `secret` and cannot be decrypted by the relay.

**Custom relay example**

```rust
# use pubky::{Pubky, PubkyAuthFlow, Capabilities, AuthFlowKind};
# async fn custom_relay() -> pubky::Result<()> {
let pubky = Pubky::new()?;
let caps = Capabilities::builder().read("pub/example.com/").finish();
let auth_flow = PubkyAuthFlow::builder(&caps, AuthFlowKind::signin())
    .client(pubky.client().clone())
    .relay(url::Url::parse("http://localhost:8080/link/")?) // your relay
    .start()?;
# Ok(()) }
```

> Tip: reuse `pubky.client()` when customising the relay so the flow shares
> TLS and pkarr configuration with the rest of your application.

## Features

- `json`: enable `Storage` helpers (`.get_json()` / `.put_json()`) and serde on certain types.

```toml
# Cargo.toml
[dependencies]
pubky = { version = "x.y.z", features = ["json"] }
```

## Testing locally

Spin up an ephemeral testnet (DHT + homeserver + relay) and run your tests fully offline:

```rust no_run
# use pubky_testnet::{EphemeralTestnet, pubky::Keypair};
# async fn test() -> pubky_testnet::pubky::Result<()> {

let testnet = EphemeralTestnet::start().await.unwrap();
let homeserver  = testnet.homeserver_app();
let pubky = testnet.sdk()?;

let signer = pubky.signer(Keypair::random());
let session  = signer.signup(&homeserver.public_key().into(), None).await?;

session.storage().put("/pub/my-cool-app/hello.txt", "hi").await?;
let s = session.storage().get("/pub/my-cool-app/hello.txt").await?.text().await?;
assert_eq!(s, "hi");

# Ok(()) }
```

## Keypair and Session persistence

Encrypted Keypair secrets (`.pkarr`):

```rust no_run
use pubky::Pubky;
# fn run() -> pubky::Result<()> {
let pubky = Pubky::new()?;
let signer = pubky.signer_from_recovery_file("/path/to/alice.pkarr", "passphrase")?;
# Ok(()) }
```

Session secrets (`.sess`):

```rust no_run
use pubky::{Pubky, Keypair};
# async fn run() -> pubky::Result<()> {
let pubky = Pubky::new()?;
let keypair = Keypair::random();
let session = pubky.signer(keypair).signin().await?;
session.write_secret_file("alice.sess").unwrap();
let restored = pubky.session_from_file("alice.sess").await?;

# let _ = std::fs::remove_file("alice.sess");
# Ok(()) }
```

> Security: the `.sess` secret is a **bearer token**. Anyone holding it can act as the user within the granted capabilities. Treat it like a password.

## Example code

Check more [examples](https://github.com/pubky/pubky-core/tree/main/examples) using the Pubky SDK.

## JS bindings

Find a wrapper of this crate using `wasm_bindgen` in [npmjs.com](https://www.npmjs.com/package/@synonymdev/pubky). Or build on `pubky-sdk` codebase under `pubky-sdk/bindings/js`.

---

**License:** MIT
**Relay:** [https://httprelay.io](https://httprelay.io) (open source; run your own for production)
