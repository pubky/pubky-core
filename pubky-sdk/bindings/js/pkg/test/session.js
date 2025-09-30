import test from "tape";
import { Pubky, PublicKey, Keypair } from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

/**
 * Basic auth lifecycle:
 *  - signer -> signup -> session (cookie stored)
 *  - write succeeds while authenticated
 *  - signout invalidates cookie
 *  - raw PUT without a valid cookie returns 401
 *  - signer -> signin -> new session; writes succeed again
 */
test("Auth: basic", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();

  // 1) Signup -> valid session
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
  t.ok(session, "signup returned a session");

  const userPk = session.info().publicKey().z32();
  const path = "/pub/example.com/auth-basic.txt";

  // 2) Write while logged in
  await session.storage().putText(path, "hello world");

  // 3) Sign out (server clears cookie)
  await session.signout();

  // 4) Unauthorized write should now fail with 401
  const client = sdk.client();
  const url = `pubky://${userPk}${path}`;
  const res401 = await client.fetch(url, {
    method: "PUT",
    body: "should fail",
    credentials: "include",
  });
  t.equal(res401.status, 401, "PUT without session returns 401");

  // 5) Sign in again (local key proves identity)
  const session2 = await signer.signin();
  t.ok(session2, "signin returned a new session");

  // 6) Write succeeds again
  await session2.storage().putText(path, "hello again");

  t.end();
});

/**
 * Multi-user cookie isolation in one process:
 *  - signup Alice and Bob (both cookies stored)
 *  - generic client.fetch PUT with credentials:include writes under the correct user's host
 *  - signout Bob; Alice remains authenticated and can still write
 *  - Bob can no longer write (401)
 */
test("Auth: multi-user (cookies)", async (t) => {
  const sdk = Pubky.testnet();

  const client = sdk.client();
  const alice = sdk.signer(Keypair.random());
  const bob = sdk.signer(Keypair.random());

  const aliceSignup = await createSignupToken();
  const bobSignup = await createSignupToken();

  // 1) Signup Alice
  const aliceSession = await alice.signup(HOMESERVER_PUBLICKEY, aliceSignup);
  t.ok(aliceSession, "alice signed up");
  const alicePk = aliceSession.info().publicKey().z32();

  // 2) Signup Bob (cookie jar now holds both sessions)
  const bobSession = await bob.signup(HOMESERVER_PUBLICKEY, bobSignup);
  t.ok(bobSession, "bob signed up");
  const bobPk = bobSession.info().publicKey().z32();

  // 3) Write for Bob via generic client.fetch
  {
    const url = `pubky://${bobPk}/pub/example.com/multi-bob.txt`;
    const r = await client.fetch(url, {
      method: "PUT",
      body: "bob-data",
      credentials: "include",
    });
    t.ok(r.ok, "bob can write");
  }

  // 4) Alice still authenticated and can write too
  {
    const url = `pubky://${alicePk}/pub/example.com/multi-alice.txt`;
    const r = await client.fetch(url, {
      method: "PUT",
      body: "alice-data",
      credentials: "include",
    });
    t.ok(r.ok, "alice can still write");
  }

  // 5) Sign out Bob
  await bobSession.signout();

  // 6) Alice still authenticated after Bob signs out
  {
    const url = `pubky://${alicePk}/pub/example.com/multi-alice-2.txt`;
    const r = await client.fetch(url, {
      method: "PUT",
      body: "alice-still-ok",
      credentials: "include",
    });
    t.ok(r.ok, "alice still can write after bob signout");
  }

  // 7) Bob can no longer write
  {
    const url = `pubky://${bobPk}/pub/example.com/multi-bob-2.txt`;
    const r = await client.fetch(url, {
      method: "PUT",
      body: "should-fail",
      credentials: "include",
    });
    t.equal(r.status, 401, "bob write fails after signout");
  }

  t.end();
});
