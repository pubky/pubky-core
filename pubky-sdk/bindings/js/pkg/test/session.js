import test from "tape";
import { AuthFlow, Client, Signer, PublicKey, useTestnet } from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

test("Auth: basic", async (t) => {
  useTestnet();

  const signer = Signer.random();
  const signupToken = await createSignupToken();

  // 1) Signup -> we have a valid session (cookie stored)
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
  t.ok(session, "signup returned a session");

  const userPk = session.info().publicKey().z32();
  const path = "/pub/example.com/auth-basic.txt";

  // 2) Write while logged in (via SessionStorage)
  await session.storage().putText(path, "hello world");

  // 3) Sign out (invalidates cookie)
  await session.signout();

  // 4) Verify unauthorized write now fails (no session)
  const client = Client.testnet();
  const url = `pubky://${userPk}${path}`;
  const res401 = await client.fetch(url, {
    method: "PUT",
    body: "should fail",
    credentials: "include",
  });
  t.equal(res401.status, 401, "PUT without session returns 401");

  // 5) Sign in again (re-establish session)
  const session2 = await signer.signin();
  t.ok(session2, "signin returned a new session");

  // 6) Write succeeds again
  await session2.storage().putText(path, "hello again");

  t.end();
});

test("Auth: multi-user (cookies)", async (t) => {
  useTestnet();

  const client = Client.testnet();
  const alice = Signer.random();
  const bob = Signer.random();

  const aliceSignup = await createSignupToken();
  const bobSignup = await createSignupToken();

  // 1) Signup Alice
  const aliceSession = await alice.signup(HOMESERVER_PUBLICKEY, aliceSignup);
  t.ok(aliceSession, "alice signed up");
  const alicePk = aliceSession.info().publicKey().z32();

  // 2) Signup Bob (same cookie jar should now hold *both* sessions)
  const bobSession = await bob.signup(HOMESERVER_PUBLICKEY, bobSignup);
  t.ok(bobSession, "bob signed up");
  const bobPk = bobSession.info().publicKey().z32();

  // 3) Write for Bob using generic client.fetch (credentials: include)
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