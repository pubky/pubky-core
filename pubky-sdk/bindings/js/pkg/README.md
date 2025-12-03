# Pubky

JS/WASM SDK for [Pubky](https://github.com/pubky/pubky-core).

Works in browsers and Node 20+.

## Install

```bash
npm install @synonymdev/pubky
```

> **Node**: requires Node v20+ (undici fetch, WebCrypto).

Module system + TS types: ESM and CommonJS both supported; TypeScript typings generated via tsify are included. Use `import { Pubky } from "@synonymdev/pubky"` (ESM) or `const { Pubky } = require("@synonymdev/pubky")` (CJS).

## Getting Started

```js
import { Pubky, PublicKey, Keypair, AuthFlowKind } from "@synonymdev/pubky";

// Initiate a Pubky SDK facade wired for default mainnet Pkarr relays.
const pubky = new Pubky(); // or: const pubky = Pubky.testnet(); for localhost testnet.

// 1) Create random user keys and bind to a new Signer.
const keypair = Keypair.random();
const signer = pubky.signer(keypair);

// 2) Sign up at a homeserver (optionally with an invite)
const homeserver = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
);
const signupToken = "<your-invite-code-or-null>";
const session = await signer.signup(homeserver, signupToken);

// 3) Write a public JSON file (session-scoped storage uses cookies automatically)
const path = "/pub/example.com/hello.json";
await session.storage.putJson(path, { hello: "world" });

// 4) Read it publicly (no auth needed)
const userPk = session.info.publicKey.z32();
const addr = `pubky${userPk}/pub/example.com/hello.json`;
const json = await pubky.publicStorage.getJson(addr); // -> { hello: "world" }

// 5) Authenticate on a 3rd-party app
const authFlow = pubky.startAuthFlow("/pub/my-cool-app/:rw", AuthFlowKind::signin()); // require permissions to read and write into `my.app`
renderQr(authFlow.authorizationUrl); // show to user
const session = await authFlow.awaitApproval();
```

Find here [**ready-to-run examples**](https://github.com/pubky/pubky-core/tree/main/examples).

### Initialization & events

The npm package bundles the WebAssembly module and **initializes it before exposing any APIs**. This avoids the common wasm-pack pitfall where events fire before the module finishes instantiating. Long-polling flows such as `authFlow.awaitApproval()` or `authFlow.tryPollOnce()` only start their relay calls after the underlying module is ready, so you won't miss approvals while the bundle is loading.

### Reuse a single facade across your app

Use a shared `Pubky` (e.g, via context or prop drilling) instead of constructing one per request. This avoids reinitializing transports and keeps the same client available for repeated usage.

## API Overview

Use `new Pubky()` to quickly get any flow started:

```js
import { Pubky, Keypair, AuthFlowKind } from "@synonymdev/pubky";

// Mainnet (default relays)
const pubky = new Pubky();

// Local testnet wiring (Pkarr + HTTP mapping).
// Omit the argument for "localhost".
const pubkyLocal = Pubky.testnet("localhost");

// Signer (bind your keypair to a new Signer actor)
const signer = pubky.signer(Keypair.random());

// Pubky Auth flow (with capabilities)
const authFlow = pubky.startAuthFlow("/pub/my-cool-app/:rw", AuthFlowKind::signin());

// Public storage (read-only)
const publicStorage = pubky.publicStorage;

// Pkdns resolver
const pkdns = pubky.getHomeserverOf(publicKey);

// Optional: raw HTTP client for advanced use
const client = pubky.client;
```

### Client (HTTP bridge)

```js
import { Client, resolvePubky } from "@synonymdev/pubky";

const client = new Client(); // or: pubky.client.fetch(); instead of constructing a client manually

// Convert the identifier into a transport URL before fetching.
const url = resolvePubky("pubky<pubky>/pub/example.com/file.txt");
const res = await client.fetch(url);
```

---

### Keys

```js
import { Keypair, PublicKey } from "@synonymdev/pubky";

const keypair = Keypair.random();
const pubkey = keypair.publicKey;

// z-base-32 roundtrip
const parsed = PublicKey.from(pubkey.z32());
```

#### Recovery file (encrypt/decrypt root secret)

```js
// Encrypt to recovery file (Uint8Array)
const recoveryFile = keypair.createRecoveryFile("strong passphrase");

// Decrypt back into a Keypair
const restored = Keypair.fromRecoveryFile(recoveryFile, "strong passphrase");

// Build a Signer from a recovered key
const signer = pubky.signer(restored);
```

- keypair: An instance of [Keypair](#keypair).
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: A recovery file with a spec line and an encrypted secret key.

---

### Signer & Session

```js
import { Pubky, PublicKey, Keypair } from "@synonymdev/pubky";

const pubky = new Pubky();

const keypair = Keypair.random();
const signer = pubky.signer(keypair);

const homeserver = PublicKey.from("8pinxxgq…");
const session = await signer.signup(homeserver, /* invite */ null);

const session2 = await signer.signin(); // fast, prefer this; publishes PKDNS in background
const session3 = await signer.signinBlocking(); // slower but safer; waits for PKDNS publish

await session.signout(); // invalidates server session
```

**Session details**

```js
const userPk = session.info.publicKey.z32(); // -> PublicKey as z32 string
const caps = session.info.capabilities; // -> string[] permissions and paths

const storage = session.storage; // -> This User's storage API (absolute paths)
```

**Approve a pubkyauth request URL**

```js
await signer.approveAuthRequest("pubkyauth:///?caps=...&secret=...&relay=...");
```

---

### AuthFlow (pubkyauth)

End-to-end auth (3rd-party app asks a user to approve via QR/deeplink, E.g. Pubky Ring).

```js
import { Pubky, AuthFlowKind } from "@synonymdev/pubky";
const pubky = new Pubky();

// Comma-separated capabilities string
const caps = "/pub/my-cool-app/:rw,/pub/another-app/folder/:w";

// Optional relay; defaults to Synonym-hosted relay if omitted
const relay = "https://httprelay.pubky.app/link/"; // optional (defaults to this)

// Start the auth polling
const flow = pubky.startAuthFlow(caps, AuthFlowKind::signin(), relay);

renderQr(flow.authorizationUrl); // show to user

// Blocks until the signer approves; returns a ready Session
const session = await flow.awaitApproval();
```

#### Validate and normalize capabilities

If you accept capability strings from user input (forms, CLI arguments, etc.),
use `validateCapabilities` before calling `startAuthFlow`. The helper returns a
normalized string (ordering actions like `:rw`) and throws a structured error
when the input is malformed.

```js
import { Pubky, validateCapabilities, AuthFlowKind } from "@synonymdev/pubky";

const pubky = new Pubky();

const rawCaps = formData.get("caps");

try {
  const caps = validateCapabilities(rawCaps ?? "");
  const flow = pubky.startAuthFlow(caps, AuthFlowKind::signin());
  renderQr(flow.authorizationUrl);
  const session = await flow.awaitApproval();
  // ...
} catch (error) {
  if (error.name === "InvalidInput") {
    surfaceValidationError(error.message);
    return;
  }
  throw error;
}
```

On invalid input, `validateCapabilities` throws a `PubkyError` with
`{ name: "InvalidInput", message: "Invalid capability entries: …" }`, so you can
surface precise feedback to the user.

#### Http Relay & reliability

- If you don’t specify a relay, `PubkyAuthFlow` defaults to a Synonym-hosted relay. If that relay is down, logins won’t complete.
- For production and larger apps, run **your own http relay** (MIT, Docker): [https://httprelay.io](https://httprelay.io).
  The channel is derived as `base64url(hash(secret))`; the token is end-to-end encrypted with the `secret` and cannot be decrypted by the relay.

---

### Storage

#### PublicStorage (read-only)

```js
const pub = pubky.publicStorage;

// Reads
const response = await pub.get(
  `pubky${userPk.z32()}/pub/example.com/data.json`
); // -> Response (stream it)
await pub.getJson(`pubky${userPk.z32()}/pub/example.com/data.json`);
await pub.getText(`pubky${userPk.z32()}/pub/example.com/readme.txt`);
await pub.getBytes(`pubky${userPk.z32()}/pub/example.com/icon.png`); // Uint8Array

// Metadata
await pub.exists(`pubky${userPk.z32()}/pub/example.com/foo`); // boolean
await pub.stats(`pubky${userPk.z32()}/pub/example.com/foo`); // { content_length, content_type, etag, last_modified } | null

// List directory (addressed path "<pubky>/pub/.../") must include trailing `/`.
// list(addr, cursor=null|suffix|fullUrl, reverse=false, limit?, shallow=false)
await pub.list(
  `pubky${userPk.z32()}/pub/example.com/`,
  null,
  false,
  100,
  false
);
```

Use `get()` when you need the raw `Response` for streaming or custom parsing.

#### SessionStorage (read/write; uses cookies)

```js
const s = session.storage;

// Writes
await s.putJson("/pub/example.com/data.json", { ok: true });
await s.putText("/pub/example.com/note.txt", "hello");
await s.putBytes("/pub/example.com/img.bin", new Uint8Array([1, 2, 3]));

// Reads
const response = await s.get("/pub/example.com/data.json"); // -> Response (stream it)
await s.getJson("/pub/example.com/data.json");
await s.getText("/pub/example.com/note.txt");
await s.getBytes("/pub/example.com/img.bin");

// Metadata
await s.exists("/pub/example.com/data.json");
await s.stats("/pub/example.com/data.json");

// Listing (session-scoped absolute dir)
await s.list("/pub/example.com/", null, false, 100, false);

// Delete
await s.delete("/pub/example.com/data.json");
```

`get()` exposes the underlying `Response`, which is handy for streaming bodies or inspecting headers before consuming content.

Path rules:

- Session storage uses **absolute** paths like `"/pub/app/file.txt"`.
- Public storage uses **addressed** form `pubky<user>/pub/app/file.txt` (preferred) or `pubky://<user>/...`.

**Convention:** put your app’s public data under a domain-like folder in `/pub`, e.g. `/pub/my-new-app/`.

---

### PKDNS (Pkarr)

Resolve or publish `_pubky` records.

```js
import { Pubky, PublicKey, Keypair } from "@synonymdev/pubky";

const pubky = new Pubky();

// Read-only resolver
const homeserver = await pubky.getHomeserverOf(PublicKey.from("<user-z32>")); // string | undefined

// With keys (signer-bound)
const signer = pubky.signer(Keypair.random());

// Republish if missing or stale (reuses current host unless overridden)
await signer.pkdns.publishHomeserverIfStale();
// Or force an override now:
await signer.pkdns.publishHomeserverForce(/* optional override homeserver*/);
// Resolve your own homeserver:
await signer.pkdns.getHomeserver();
```

## Logging

The SDK ships with a WASM logger that bridges Rust `log` output into the browser or Node console. Call `setLogLevel` **once at application start**, before constructing `Pubky` or other SDK actors, to choose how verbose the logs should be.

```js
import { setLogLevel } from "@synonymdev/pubky";

setLogLevel("debug"); // "error" | "warn" | "info" | "debug" | "trace"
```

If the logger is already initialized, calling `setLogLevel` again will throw. Pick the most verbose level (`"debug"` or `"trace"`) while developing to see pkarr resolution, network requests and storage operations in the console.

#### Resolve `pubky` identifiers into transport URLs

Use `resolvePubky()` when you need to feed an addressed resource into a raw HTTP client:

```js
import { resolvePubky } from "@synonymdev/pubky";

const identifier =
  "pubkyoperrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG";
const url = resolvePubky(identifier);
// -> "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0033X02JAN0SG"
```

Both `pubky<pk>/…` (preferred) and `pubky://<pk>/…` resolve to the same HTTPS endpoint.

---

## WASM memory (`free()` helpers)

`wasm-bindgen` generates `free()` methods on exported classes (for example `Pubky`, `AuthFlow` `PublicKey`). JavaScript's GC eventually releases the underlying Rust structs on its own, but calling `free()` lets you drop them **immediately** if you are creating many short-lived instances (e.g. in a long-running worker). It is safe to skip manual frees in typical browser or Node apps.

---

## Errors

All async methods throw a structured `PubkyError`:

```ts
interface PubkyError extends Error {
  name:
    | "RequestError" // network/server/validation/JSON
    | "InvalidInput"
    | "AuthenticationError"
    | "PkarrError"
    | "InternalError";
  message: string;
  data?: unknown; // structured context when available (e.g. { statusCode: number })
}
```

Example:

```js
try {
  await publicStorage.getJson(`${pk}/pub/example.com/missing.json`);
} catch (e) {
  const error = e as PubkyError;
  if (
    error.name === "RequestError" &&
    typeof error.data === "object" &&
    error.data !== null &&
    "statusCode" in error.data &&
    typeof (error.data as { statusCode?: number }).statusCode === "number" &&
    (error.data as { statusCode?: number }).statusCode === 404
  ) {
    // handle not found
  }
}
```

## Browser environment notes

- Keep the Pubky client UI and the homeserver on the **same origin family** (both local or both remote). Browsers partition cookies by scheme/host, and cross-site requests (e.g., http://localhost calling https://staging…​) can silently drop or cache `SameSite`/`Secure` session cookies.
- If you must mix environments, use a reverse proxy so the browser always talks to one consistent origin (or disable caching via devtools and clear cookies between switches).
- When troubleshooting auth/session caching: open a fresh incognito window, clear site data for the target origin, and verify the request includes credentials.

---

## Local Test & Development

For test and development, you can run a local homeserver in a test network.

1. Install Rust (for wasm and testnet builds):

```bash
curl https://sh.rustup.rs -sSf | sh
```

2. Install and run the local testnet:

```bash
cargo install pubky-testnet
pubky-testnet
```

3. Point the SDK at testnet:

```js
import { Pubky } from "@synonymdev/pubky";

const pubky = Pubky.testnet(); // defaults to localhost
// or: const pubky = Pubky.testnet("testnet-host");  // custom host (e.g. Docker bridge)
```

---

MIT © Synonym
