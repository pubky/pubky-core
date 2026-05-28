#!/usr/bin/env node
// End-to-end testnet roundtrip: signup -> signin -> write -> read.
import { Pubky, Keypair, PublicKey } from "@synonymdev/pubky";

// This is the default testnet homeserver. It comes from the secret `00000...` (bits).
const TESTNET_HOMESERVER =
  "pubky8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";

// 1) Build Pubky SDK facade for local testnet host
const pubky = Pubky.testnet();

// 2) Make a random keypair, bind it to a signer and sign up on the given homeserver
const keypair = Keypair.random();
const signer = pubky.signer(keypair);
const homeserver = PublicKey.from(TESTNET_HOMESERVER);
await signer.signup(homeserver, null);

// 3) Sign in to create a grant-backed session for storage access
const session = await signer.signin("my-cool-app.example");
console.log("Signed up succeeded for user:", session.info.publicKey.toString());

// 4) Write then read a file under /pub/<your.app>/
const path = "/pub/my-cool-app/hello.txt";
await session.storage.putText(path, "hi");
console.log("Data write succeeded on path:", path);

const roundtrip = await session.storage.getText(path);
console.log("Roundtrip succeeded, response data:", roundtrip);
