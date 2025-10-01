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
import { Pubky, PublicKey, Keypair } from "@synonymdev/pubky";

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
await session.storage().putJson(path, { hello: "world" });

// 4) Read it publicly (no auth needed)
const userPk = session.info().publicKey();
const addr = `${userPk.z32()}/pub/example.com/hello.json`;
const json = await pubky.publicStorage().getJson(addr); // -> { hello: "world" }
```

Find here [**ready-to-run examples**](https://github.com/pubky/pubky-core/tree/main/examples).

## API Overview

Use `new Pubky()` to quickly get any flow started:

```js
import { Pubky, Keypair } from "@synonymdev/pubky";

// Mainnet (default relays)
const pubky = new Pubky();

// Local testnet wiring (Pkarr + HTTP mapping).
// Omit the argument for "localhost".
const pubkyLocal = Pubky.testnet("localhost");

// Signer (bind your keypair to a new Signer actor)
const signer = pubky.signer(Keypair.random());

// Public storage (read-only)
const publicStorage = pubky.publicStorage();

// PKDNS resolver (read-only)
const pkdns = pubky.pkdns();

// Optional: raw HTTP client for advanced use
const client = pubky.client();
```

### Client (HTTP bridge)

```js
import { Client } from "@synonymdev/pubky";

const client = new Client(); // or: pubky.client(); instead of constructing a client manually

// Works with both pubky:// and http(s)://
const res = await client.fetch("pubky://<pubky>/pub/example.com/file.txt");
```

---

### Keys

```js
import { Keypair, PublicKey } from "@synonymdev/pubky";

const keypair = Keypair.random();
const pubkey = keypair.publicKey();

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
const info = session.info();
const userPk = info.publicKey(); // -> PublicKey
const caps = info.capabilities(); // -> string[]

const storage = session.storage(); // -> SessionStorage (absolute paths)
```

**Approve a pubkyauth request URL**

```js
await signer.approveAuthRequest("pubkyauth:///?caps=...&secret=...&relay=...");
```

---

### AuthFlow (pubkyauth)

End-to-end auth (3rd-party app asks a user to approve via QR/deeplink, E.g. Pubky Ring).

```js
import { Pubky } from "@synonymdev/pubky";
const pubky = new Pubky();

// Comma-separated capabilities string
const caps = "/pub/my.app/:rw,/pub/another.app/folder/:w";

// Optional relay; defaults to Synonym-hosted relay if omitted
const relay = "https://httprelay.pubky.app/link/"; // optional (defaults to this)

// Start the auth polling
const flow = pubky.startAuthFlow(caps, relay);

renderQr(flow.authorizationUrl()); // show to user

// Blocks until the signer approves; returns a ready Session
const session = await flow.awaitApproval();
```

#### Http Relay & reliability

- If you don’t specify a relay, `PubkyAuthFlow` defaults to a Synonym-hosted relay. If that relay is down, logins won’t complete.
- For production and larger apps, run **your own http relay** (MIT, Docker): [https://httprelay.io](https://httprelay.io).
  The channel is derived as `base64url(hash(secret))`; the token is end-to-end encrypted with the `secret` and cannot be decrypted by the relay.

---

### Storage

#### PublicStorage (read-only)

```js
const pub = pubky.publicStorage();

// Reads
await pub.getJson(`${userPk.z32()}/pub/example.com/data.json`);
await pub.getText(`${userPk.z32()}/pub/example.com/readme.txt`);
await pub.getBytes(`${userPk.z32()}/pub/example.com/icon.png`); // Uint8Array

// Metadata
await pub.exists(`${userPk.z32()}/pub/example.com/foo`); // boolean
await pub.stats(`${userPk.z32()}/pub/example.com/foo`); // { content_length, content_type, etag, last_modified } | null

// List directory (addressed path "<pubky>/pub/.../") must include trailing `/`.
// list(addr, cursor=null|suffix|fullUrl, reverse=false, limit?, shallow=false)
await pub.list(`${userPk.z32()}/pub/example.com/`, null, false, 100, false);
```

#### SessionStorage (read/write; uses cookies)

```js
const s = session.storage();

// Writes
await s.putJson("/pub/example.com/data.json", { ok: true });
await s.putText("/pub/example.com/note.txt", "hello");
await s.putBytes("/pub/example.com/img.bin", new Uint8Array([1, 2, 3]));

// Reads
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

Path rules:

- Session storage uses **absolute** paths like `"/pub/app/file.txt"`.
- Public storage uses **addressed** form `<user>/pub/app/file.txt` (or `pubky://<user>/...`).

**Convention:** put your app’s public data under a domain-like folder in `/pub`, e.g. `/pub/mycoolnew.app/`.

---

### PKDNS (Pkarr)

Resolve or publish `_pubky` records.

```js
import { Pubky, PublicKey, Keypair } from "@synonymdev/pubky";

const pubky = new Pubky();

// Read-only resolver
const resolver = pubky.pkdns();
const homeserver = await resolver.getHomeserverOf(PublicKey.from("<user-z32>")); // string | undefined

// With keys (signer-bound)
const signer = pubky.signer(Keypair.random());

// Republish if missing or stale (reuses current host unless overridden)
await signer.pkdns().publishHomeserverIfStale();
// Or force an override now:
await signer.pkdns().publishHomeserverForce(/* optional override homeserver*/);
```

---

## Errors

All async methods throw a structured `PubkyJsError`:

```ts
type PubkyJsError = {
  name:
    | "RequestError" // network/server/validation/JSON
    | "InvalidInput"
    | "AuthenticationError"
    | "PkarrError"
    | "InternalError";
  message: string;
  statusCode?: number; // present for HTTP server errors (4xx/5xx)
};
```

Example:

```js
try {
  await publicStorage.getJson(`${pk}/pub/example.com/missing.json`);
} catch (e) {
  if (e.name === "RequestError" && e.statusCode === 404) {
    // handle not found
  }
}
```

---

## Local Test & Development

For test and development, you can run a local homeserver in a test network.

1. Install Rust (for wasm builds):

```bash
curl https://sh.rustup.rs -sSf | sh
```

2. Run the local testnet:

```bash
npm run testnet
```

3. Point the SDK at testnet:

```js
import { Pubky } from "@synonymdev/pubky";

const pubky = Pubky.testnet(); // defaults to localhost
// or: const pubky = Pubky.testnet("testnet-host");  // custom host (e.g. Docker bridge)
```

---

MIT © Synonym
