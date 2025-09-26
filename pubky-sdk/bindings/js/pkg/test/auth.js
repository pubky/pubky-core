// import test from "tape";

// import { Client, Keypair, PublicKey, setLogLevel } from "../index.cjs";
// import { createSignupToken } from "./utils.js";

// const HOMESERVER_PUBLICKEY = PublicKey.from(
//   "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
// );
// const TESTNET_HTTP_RELAY = "http://localhost:15412/link";

// test("Auth: basic", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();

//   const signupToken = await createSignupToken(client);

//   // Use the received token to sign up.
//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   const session = await client.session(publicKey);
//   t.ok(session, "signup");

//   {
//     await client.signout(publicKey);

//     const session = await client.session(publicKey);
//     t.notOk(session, "signout");
//   }

//   {
//     await client.signin(keypair);

//     const session = await client.session(publicKey);
//     t.ok(session, "signin");
//   }
// });

// test("Auth: multi-user (cookies)", async (t) => {
//   const client = Client.testnet();

//   const alice = Keypair.random();
//   const bob = Keypair.random();

//   const aliceSignupToken = await createSignupToken(client);
//   const bobSignupToken = await createSignupToken(client);

//   await client.signup(alice, HOMESERVER_PUBLICKEY, aliceSignupToken);

//   let session = await client.session(alice.publicKey());
//   t.ok(session, "signup");

//   {
//     await client.signup(bob, HOMESERVER_PUBLICKEY, bobSignupToken);

//     const session = await client.session(bob.publicKey());
//     t.ok(session, "signup");
//   }

//   session = await client.session(alice.publicKey());
//   t.is(
//     session.pubky().z32(),
//     alice.publicKey().z32(),
//     "alice is still signed in",
//   );

//   await client.signout(bob.publicKey());

//   session = await client.session(alice.publicKey());
//   t.is(
//     session.pubky().z32(),
//     alice.publicKey().z32(),
//     "alice is still signed in after signout of bob",
//   );
// });

// test("Auth: 3rd party signin", async (t) => {
//   let keypair = Keypair.random();
//   let pubky = keypair.publicKey().z32();

//   // Third party app side
//   let capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
//   let client = Client.testnet();
//   let authRequest = client.authRequest(TESTNET_HTTP_RELAY, capabilities);

//   let pubkyauthUrl = authRequest.url();
//   let pubkyauthResponse = authRequest.response();

//   if (globalThis.document) {
//     // Skip `sendAuthToken` in browser
//     // TODO: figure out why does it fail in browser unit tests
//     // but not in real browser (check pubky-auth-widget.js commented part)
//     return;
//   }

//   // Authenticator side
//   {
//     let client = Client.testnet();

//     const signupToken = await createSignupToken(client);

//     await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//     await client.sendAuthToken(keypair, pubkyauthUrl);
//   }

//   let authedPubky = await pubkyauthResponse;

//   t.is(authedPubky.z32(), pubky);

//   let session = await client.session(authedPubky);
//   t.deepEqual(session.capabilities(), capabilities.split(","));
// });

// test("getHomeserver not found", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();

//   try {
//     let homeserver = await client.getHomeserver(publicKey);
//     t.fail("getHomeserver should NOT be found.");
//   } catch (e) {
//     t.pass("getHomeserver should NOT be found.");
//   }
// });

// function sleep(ms) {
//   return new Promise((resolve) => setTimeout(resolve, ms));
// }

// test("getHomeserver success", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   let homeserver = await client.getHomeserver(publicKey);
//   t.is(homeserver.z32(), HOMESERVER_PUBLICKEY.z32(), "homeserver is correct");
// });
