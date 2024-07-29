# Pubky

JavaScript implementation of [Pubky](https://github.com/pubky/pubky).

## Install

```bash
npm install @synonymdev/pubky
```

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

// Verify that you are signed in.
const session = await client.session(publicKey)

const publicKey = keypair.public_key();

const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

// PUT public data, by authorized client
await client.put(publicKey, "/pub/example.com/arbitrary", body);

// GET public data without signup or signin
{
    const client = new PubkyClient();

    let response = await client.get(publicKey, "/pub/example.com/arbitrary");
}
```

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

Pass the logged addresses as inputs to `PubkyClient`

```js
import { PubkyClient, PublicKey } from '../index.js'

const client = new PubkyClient().setPkarrRelays(["http://localhost:15411/pkarr"]);

let homeserver = PublicKey.from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo");
```
