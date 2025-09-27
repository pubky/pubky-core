# Pubky

WASM/JS client for the [Pubky](https://github.com/pubky/pubky-core) SDK.

- Works in browsers and Node 20+.
- First-class Signer/Session model.
- Public & session-scoped storage helpers.
- PKDNS (Pkarr) resolve/publish for homeservers.
- Simple `AuthFlow` for pubkyauth.

## Install

```bash
npm install @synonymdev/pubky
```

> **Node**: requires Node v20+ (undici fetch, WebCrypto).

## Getting Started

```js
import {
  useTestnet, // optional: point SDK at local relays/hosts
  Signer,
  PublicKey,
  PublicStorage,
} from "@synonymdev/pubky";

// (optional) local dev wiring (relays + http mapping)
useTestnet();

// 1) Create a signer (user keys)
const signer = Signer.random();

// 2) Sign up at a homeserver (optionally with an invite)
const homeserver = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
);
const signupToken = "<your-invite-token-or-null>";
const session = await signer.signup(homeserver, signupToken);

// 3) Write a public JSON file (session-scoped storage uses cookies automatically)
const path = "/pub/example.com/hello.json";
await session.storage().putJson(path, { hello: "world" });

// 4) Read it publicly (no auth)
const userPk = session.info().publicKey();
const addr = `${userPk.z32()}/pub/example.com/hello.json`;
const pub = new PublicStorage();
const json = await pub.getJson(addr); // -> { hello: "world" }
```

## API Overview

### Client (HTTP bridge)

```js
import { Client } from "@synonymdev/pubky";

const client = new Client(); // or: new Client()

// Works with both pubky:// and http(s)://
const res = await client.fetch("pubky://<pubky>/pub/example.com/file.txt");
```

**Helpers**

- `useTestnet(host = "localhost")` - sets global testnet wiring (relays + HTTP mapping).
- `Client.testnet(host?)` - returns a `Client` preconfigured for testnet.

---

### Keys

```js
import { Keypair, PublicKey } from "@synonymdev/pubky";

const kp = Keypair.random();
const pk = kp.publicKey();
const pk2 = PublicKey.from(pk.z32()); // parse from z-base-32 string
```

#### createRecoveryFile

```js
let recoveryFile = keypair.createRecoveryFile(passphrase);
```

- keypair: An instance of [Keypair](#keypair).
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: A recovery file with a spec line and an encrypted secret key.

#### toRecoveryFile

```js
let keypair = Keypair.fromRecoveryfile(recoveryFile, passphrase);
```

- recoveryFile: An instance of Uint8Array containing the recovery file blob.
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: An instance of [Keypair](#keypair).

---

### Signer & Session

```js
import { Signer } from "@synonymdev/pubky";

const signer = Signer.random(); // or new Signer(keypair)
const session = await signer.signup(homeserverPk, signupToken);
const session2 = await signer.signin(); // fast sign-in, publishes in background
const session3 = await signer.signinBlocking(); // slow sign-in, waits for homeserver publish

await session.signout(); // invalidates session on server
```

**Session details**

```js
const info = session.info();
info.publicKey(); // -> PublicKey
info.capabilities(); // -> string[]

session.storage(); // -> SessionStorage (absolute paths)
```

**Approve a pubkyauth request URL**

```js
await signer.approveAuthRequest("pubkyauth:///?caps=...&secret=...&relay=...");
```

---

### Storage

#### PublicStorage (read-only; no cookies)

```js
import { PublicStorage } from "@synonymdev/pubky";
const pub = new PublicStorage();

await pub.getJson(`${pubky}/pub/example.com/data.json`);
await pub.getText(`${pubky}/pub/example.com/readme.txt`);
await pub.getBytes(`${pubky}/pub/example.com/icon.png`); // Uint8Array

await pub.exists(`${pubky}/pub/example.com/foo`);
await pub.stats(`${pubky}/pub/example.com/foo`); // { size, etag, ... } | null

// List (prefix = "<pubky>/pub/.../"), optional cursor/reverse/limit/shallow
await pub.list(`${pubky}/pub/example.com/`, null, false, 100, false);
```

#### SessionStorage (read/write; uses cookies)

```js
const s = session.storage();

await s.putJson("/pub/example.com/data.json", { ok: true });
await s.putText("/pub/example.com/note.txt", "hello");
await s.putBytes("/pub/example.com/img.bin", new Uint8Array([1, 2, 3]));

await s.getJson("/pub/example.com/data.json");
await s.getText("/pub/example.com/note.txt");
await s.getBytes("/pub/example.com/img.bin");

await s.exists("/pub/example.com/data.json");
await s.stats("/pub/example.com/data.json");

await s.list("/pub/example.com/", null, false, 100, false);
await s.delete("/pub/example.com/data.json");
```

---

### PKDNS (Pkarr)

Resolve or publish `_pubky` records.

```js
import { Pkdns, Signer, PublicKey } from "@synonymdev/pubky";

// Read-only
const pkdns = Pkdns.new();
const host = await pkdns.getHomeserverOf(PublicKey.from("<user-z32>")); // string|null

// With keys (bound to signer)
const signer = Signer.random();
await signer.signup(homeserverPk, null);
const pkdnsWithKeys = signer.pkdns();

// Publish if missing or stale (uses the same host as the current record unless overridden)
await pkdnsWithKeys.publishHomeserverIfStale(); // optional override: migrate homeserver to (hostPk)
await pkdnsWithKeys.publishHomeserverForce(); // optional override: migrate homeserver to (hostPk)
```

---

### AuthFlow (pubkyauth)

End-to-end auth (3rd-party app asks a user to approve via QR/deeplink, E.g. Pubky Ring).

```js
import { AuthFlow } from "@synonymdev/pubky";

const caps = "/pub/my.app/:rw"; // capabilities string
const relay = "https://httprelay.pubky.app/link/"; // optional (defaults to this)
const flow = AuthFlow.start(caps, relay);

renderQr(flow.authorizationUrl()); // show to user

// Blocks until approved; returns a ready Session
const session = await flow.awaitApproval();
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
import { useTestnet } from "@synonymdev/pubky";
useTestnet(); // defaults to localhost for Pkarr and Http relays.
```

---

MIT © Synonym
