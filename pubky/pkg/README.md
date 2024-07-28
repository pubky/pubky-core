# Pubky

JavaScript implementation of [Pubky](https://github.com/pubky/pubky).

## Install

```bash
npm install @synonymdev/pubky
```

## Getting started

```js
import PubkyClient from "@synonymdev/pubky";

// Initialize PubkyClient with Pkarr relay(s).
let client = new PubkyClient();

// Generate a keypair
let keypair = Keypair.random();

// Create a new account
let homeserver = PublicKey.try_from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo");

await client.signup(keypair, homeserver)
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
cd pubky/
```

Run the testnet server

```bash
cargo run --bin pubky_homeserver -- --testnet
```
