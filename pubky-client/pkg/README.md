# Pubky

JavaScript implementation of [Pubky](https://github.com/pubky/pubky-core) client.

## Table of Contents

- [Install](#install)
- [Getting Started](#getting-started)
- [API](#api)
- [Test and Development](#test-and-development)

## Install

```bash
npm install @synonymdev/pubky
```

### Prerequisites

For Nodejs, you need Node v20 or later.

## Getting started

```js
import { Client, Keypair, PublicKey } from "../index.js";

// Initialize Client with Pkarr relay(s).
let client = new Client();

// Generate a keypair
let keypair = Keypair.random();

// Create a new account
let homeserver = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
);

// Some homeservers might require a `signup_token` or to `accept_tos: bool`
await client.signup(keypair, homeserver, signup_token);

const publicKey = keypair.publicKey();

// Pubky URL
let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

// Verify that you are signed in.
const session = await client.session(publicKey);

// PUT public data, by authorized client
await client.fetch(url, {
  method: "PUT",
  body: JSON.stringify({ foo: "bar" }),
  credentials: "include",
});

// GET public data without signup or signin
{
  const client = new Client();

  let response = await client.fetch(url);
}

// Delete public data, by authorized client
await client.fetch(url, { method: "DELETE", credentials: "include " });
```

## API

### Client

#### constructor

```js
let client = new Client();
```

#### fetch

```js
let response = await client.fetch(url, opts);
```

Just like normal Fetch API, but it can handle `pubky://` urls and `http(s)://` urls with Pkarr domains.

#### signup

```js
await client.signup(keypair, homeserver, signup_token);
```

- keypair: An instance of [Keypair](#keypair).
- homeserver: An instance of [PublicKey](#publickey) representing the homeserver.
- signup_token: A homeserver could optionally ask for a valid signup token (aka, invitation code).

Returns:

- session: An instance of [Session](#session).

#### signin

```js
let session = await client.signin(keypair);
```

- keypair: An instance of [Keypair](#keypair).

Returns:

- An instance of [Session](#session).

#### signout

```js
await client.signout(publicKey);
```

- publicKey: An instance of [PublicKey](#publicKey).

#### authRequest

```js
let pubkyAuthRequest = client.authRequest(relay, capabilities);

let pubkyauthUrl = pubkyAuthRequest.url();

showQr(pubkyauthUrl);

let pubky = await pubkyAuthRequest.response();
```

Sign in to a user's Homeserver, without access to their [Keypair](#keypair), nor even [PublicKey](#publickey),
instead request permissions (showing the user pubkyauthUrl), and await a Session after the user consenting to that request.

- relay: A URL to an [HTTP relay](https://httprelay.io/features/link/) endpoint.
- capabilities: A list of capabilities required for the app for example `/pub/pubky.app/:rw,/pub/example.com/:r`.

#### sendAuthToken

```js
await client.sendAuthToken(keypair, pubkyauthUrl);
```

Consenting to authentication or authorization according to the required capabilities in the `pubkyauthUrl` , and sign and send an auth token to the requester.

- keypair: An instance of [KeyPair](#keypair)
- pubkyauthUrl: A string `pubkyauth://` url

#### session {#session-method}

```js
let session = await client.session(publicKey);
```

- publicKey: An instance of [PublicKey](#publickey).
- Returns: A [Session](#session) object if signed in, or undefined if not.

### list

```js
let response = await client.list(url, cursor, reverse, limit);
```

- url: A string representing the Pubky URL. The path in that url is the prefix that you want to list files within.
- cursor: Usually the last URL from previous calls. List urls after/before (depending on `reverse`) the cursor.
- reverse: Whether or not return urls in reverse order.
- limit: Number of urls to return.
- Returns: A list of URLs of the files in the `url` you passed.

### Keypair

#### random

```js
let keypair = Keypair.random();
```

- Returns: A new random Keypair.

#### fromSecretKey

```js
let keypair = Keypair.fromSecretKey(secretKey);
```

- secretKey: A 32 bytes Uint8array.
- Returns: A new Keypair.

#### publicKey {#publickey-method}

```js
let publicKey = keypair.publicKey();
```

- Returns: The [PublicKey](#publickey) associated with the Keypair.

#### secretKey

```js
let secretKey = keypair.secretKey();
```

- Returns: The Uint8array secret key associated with the Keypair.

### PublicKey

#### from

```js
let publicKey = PublicKey.from(string);
```

- string: A string representing the public key.
- Returns: A new PublicKey instance.

#### z32

```js
let pubky = publicKey.z32();
```

Returns: The z-base-32 encoded string representation of the PublicKey.

### Session

#### pubky

```js
let pubky = session.pubky();
```

Returns an instance of [PublicKey](#publickey)

#### capabilities

```js
let capabilities = session.capabilities();
```

Returns an array of capabilities, for example `["/pub/pubky.app/:rw"]`

### Helper functions

#### createRecoveryFile

```js
let recoveryFile = createRecoveryFile(keypair, passphrase);
```

- keypair: An instance of [Keypair](#keypair).
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: A recovery file with a spec line and an encrypted secret key.

#### createRecoveryFile

```js
let keypair = decryptRecoveryfile(recoveryFile, passphrase);
```

- recoveryFile: An instance of Uint8Array containing the recovery file blob.
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: An instance of [Keypair](#keypair).

## Test and Development

For test and development, you can run a local homeserver in a test network.

If you don't have Cargo Installed, start by installing it:

```bash
curl https://sh.rustup.rs -sSf | sh
```

Clone the Pubky repository:

```bash
git clone https://github.com/pubky/pubky
cd pubky-client/pkg
```

Run the local testnet server

```bash
npm run testnet
```

Use the logged addresses as inputs to `Client`

```js
import { Client } from "../index.js";

const client = Client().testnet();
```
