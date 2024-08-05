# Pubky

JavaScript implementation of [Pubky](https://github.com/pubky/pubky).

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
import { PubkyClient, Keypair, PublicKey } from '../index.js'

// Initialize PubkyClient with Pkarr relay(s).
let client = new PubkyClient();

// Generate a keypair
let keypair = Keypair.random();

// Create a new account
let homeserver = PublicKey.from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo");

await client.signup(keypair, homeserver)

const publicKey = keypair.publicKey();

// Pubky URL
let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

// Verify that you are signed in.
const session = await client.session(publicKey)

const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

// PUT public data, by authorized client
await client.put(url, body);

// GET public data without signup or signin
{
    const client = new PubkyClient();

    let response = await client.get(url);
}

// Delete public data, by authorized client
await client.delete(url);
```

## API

### PubkyClient

#### constructor
```js
let client = new PubkyClient()
```

#### createRecoveryFile
```js
let recoveryFile = PubkyClient.createRecoveryFile(keypair, passphrase)
```
- keypair: An instance of [Keypair](#keypair).
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: A recovery file with a spec line and an encrypted secret key.

#### createRecoveryFile
```js
let keypair = PubkyClient.decryptRecoveryfile(recoveryFile, passphrase)
```
- recoveryFile: An instance of Uint8Array containing the recovery file blob.
- passphrase: A utf-8 string [passphrase](https://www.useapassphrase.com/).
- Returns: An instance of [Keypair](#keypair).

#### signup
```js
await client.signup(keypair, homeserver)
```
- keypair: An instance of [Keypair](#keypair).
- homeserver: An instance of [PublicKey](#publickey) representing the homeserver.

#### session
```js
let session = await client.session(publicKey)
```
- publicKey: An instance of [PublicKey](#publickey).
- Returns: A session object if signed in, or undefined if not.

#### put
```js
let response = await client.put(url, body);
```
- url: A string representing the Pubky URL.
- body: A Buffer containing the data to be stored.

### get
```js
let response = await client.get(url)
```
- url: A string representing the Pubky URL.
- Returns: A response object containing the requested data.

### delete

```js
let response = await client.delete(url);
```
- url: A string representing the Pubky URL.

### list
```js
let response = await client.list(url, cursor, reverse, limit)
```
- url: A string representing the Pubky URL. The path in that url is the prefix that you want to list files within.
- cursor: Usually the last URL from previous calls. List urls after/before (depending on `reverse`) the cursor.
- reverse: Whether or not return urls in reverse order.
- limit: Number of urls to return.
- Returns: A list of URLs of the files in the `url` you passed.

### Keypair

#### random
```js
let keypair = Keypair.random()
```
- Returns: A new random Keypair.

#### fromSecretKey
```js
let keypair = Keypair.fromSecretKey(secretKey)
```
- secretKey: A 32 bytes Uint8array.
- Returns: A new Keypair.


#### publicKey
```js
let publicKey = keypair.publicKey()
```
- Returns: The [PublicKey](#publickey) associated with the Keypair.

#### secretKey
```js
let secretKey = keypair.secretKey()
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

## Test and Development

For test and development, you can run a local homeserver in a test network.

If you don't have Cargo Installed, start by installing it:

```bash
curl https://sh.rustup.rs -sSf | sh
```

Clone the Pubky repository:

```bash
git clone https://github.com/pubky/pubky
cd pubky/pkg
```

Run the local testnet server

```bash
npm run testnet
```

Use the logged addresses as inputs to `PubkyClient`

```js
import { PubkyClient } from '../index.js'

const client = PubkyClient().testnet();
```
