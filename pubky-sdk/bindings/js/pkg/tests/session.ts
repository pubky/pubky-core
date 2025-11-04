import test from "tape";

import {
  Keypair,
  Pubky,
  PublicKey,
  type Address,
  type Path,
} from "../index.js";
import {
  Assert,
  IsExact,
  assertPubkyError,
  createSignupToken,
} from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

type Facade = ReturnType<typeof Pubky.testnet>;
type Signer = ReturnType<Facade["signer"]>;
type SignupSession = Awaited<ReturnType<Signer["signup"]>>;
type SessionStorageType = SignupSession["storage"];
type PublicStorageType = Facade["publicStorage"];

type _StoragePutText = Assert<
  IsExact<Parameters<SessionStorageType["putText"]>, [Path, string]>
>;
type _StorageGetBytes = Assert<
  IsExact<ReturnType<SessionStorageType["getBytes"]>, Promise<Uint8Array>>
>;
type _PublicGetText = Assert<
  IsExact<ReturnType<PublicStorageType["getText"]>, Promise<string>>
>;

const PATH_AUTH_BASIC: Path = "/pub/example.com/auth-basic.txt";

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
  const userPk = session.info.publicKey.z32();

  // 2) Write while logged in
  await session.storage.putText(PATH_AUTH_BASIC, "hello world");

  // 3) Sign out (server clears cookie)
  await session.signout();

  // Info remains readable and stable after signout
  t.ok(session.info, "info getter still works after signout");
  t.equal(
    session.info.publicKey.z32(),
    userPk,
    "Session is not null pointer after signout",
  );

  // TODO: Storage access must now fail
  // It seems to return "hello world";
  // t.throws(async() => await session.storage.getText(path), "storage throws after signout");

  // Idempotent signout
  await session.signout();
  t.pass("second signout is a no-op");

  // 4) Unauthorized write should now fail with 401
  const url = `https://_pubky.${userPk}${PATH_AUTH_BASIC}`;
  const res401 = await sdk.client.fetch(url, {
    method: "PUT",
    body: "should fail",
    credentials: "include",
  });
  t.equal(res401.status, 401, "PUT without session returns 401");

  // 5) Sign in again (local key proves identity)
  const session2 = await signer.signin();
  t.ok(session2, "signin returned a new session");

  // 6) Write succeeds again
  await session2.storage.putText(PATH_AUTH_BASIC, "hello again");

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

  const alice = sdk.signer(Keypair.random());
  const bob = sdk.signer(Keypair.random());

  const aliceSignup = await createSignupToken();
  const bobSignup = await createSignupToken();

  // 1) Signup Alice
  const aliceSession = await alice.signup(HOMESERVER_PUBLICKEY, aliceSignup);
  t.ok(aliceSession, "alice signed up");
  const alicePk = aliceSession.info.publicKey.z32();

  // 2) Signup Bob (cookie jar now holds both sessions)
  const bobSession = await bob.signup(HOMESERVER_PUBLICKEY, bobSignup);
  t.ok(bobSession, "bob signed up");
  const bobPk = bobSession.info.publicKey.z32();

  // 3) Write for Bob via generic client.fetch
  {
    const url = `https://_pubky.${bobPk}/pub/example.com/multi-bob.txt`;
    const r = await sdk.client.fetch(url, {
      method: "PUT",
      body: "bob-data",
      credentials: "include",
    });
    t.ok(r.ok, "bob can write");
  }

  // 4) Alice still authenticated and can write too
  {
    const url = `https://_pubky.${alicePk}/pub/example.com/multi-alice.txt`;
    const r = await sdk.client.fetch(url, {
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
    const url = `https://_pubky.${alicePk}/pub/example.com/multi-alice-2.txt`;
    const r = await sdk.client.fetch(url, {
      method: "PUT",
      body: "alice-still-ok",
      credentials: "include",
    });
    t.ok(r.ok, "alice still can write after bob signout");
  }

  // 7) Bob can no longer write
  {
    const url = `https://_pubky.${bobPk}/pub/example.com/multi-bob-2.txt`;
    const r = await sdk.client.fetch(url, {
      method: "PUT",
      body: "should-fail",
      credentials: "include",
    });
    t.equal(r.status, 401, "bob write fails after signout");
  }

  t.end();
});

/**
 * - Have *two* valid sessions (cookies for both users in one process).
 * - Interleave writes across both users, using BOTH high-level SessionStorage (absolute paths)
 *   and low-level Client.fetch (transport URLs).
 * - Ensure each write lands under the correct user regardless of recent activity or order.
 *
 * If the WASM client ever derives `pubky-host` from a stale/global identity,
 * or the cookie jar gets mismatched, we should see 401/403 or wrong-user data.
 */
test("Auth: multi-user host isolation + stale-handle safety", async (t) => {
  const sdk = Pubky.testnet();

  // Create two users & sign them up — both cookies end up in the same jar.
  const alice = sdk.signer(Keypair.random());
  const bob = sdk.signer(Keypair.random());

  const aliceToken = await createSignupToken();
  const bobToken = await createSignupToken();

  const aliceSession = await alice.signup(HOMESERVER_PUBLICKEY, aliceToken);
  const bobSession = await bob.signup(HOMESERVER_PUBLICKEY, bobToken);

  const A = aliceSession.info.publicKey.z32();
  const B = bobSession.info.publicKey.z32();

  const readTextPublic = async (
    user: string,
    relPath: Path,
  ): Promise<string> => {
    const address = `pubky${user}${relPath}` as Address;
    return sdk.publicStorage.getText(address);
  };

  const P: Path = "/pub/example.com/owner.txt";

  // 1) Alice writes via SessionStorage (absolute path)
  await aliceSession.storage.putText(P, "alice#1");
  t.equal(
    await readTextPublic(A, P),
    "alice#1",
    "alice write visible under alice",
  );

  // 2) Bob writes via SessionStorage
  await bobSession.storage.putText(P, "bob#1");
  t.equal(await readTextPublic(B, P), "bob#1", "bob write visible under bob");

  // 3) Interleave in reverse order (ensure no global “current user” leakage)
  await bobSession.storage.putText(P, "bob#2");
  await aliceSession.storage.putText(P, "alice#2");
  t.equal(
    await readTextPublic(A, P),
    "alice#2",
    "alice second write still under alice",
  );
  t.equal(
    await readTextPublic(B, P),
    "bob#2",
    "bob second write still under bob",
  );

  // 4) Raw client.fetch using transport URLs
  {
    const urlA = `https://_pubky.${A}${P}`;
    const r = await sdk.client.fetch(urlA, {
      method: "PUT",
      body: "alice#3",
      credentials: "include",
    });
    t.ok(r.ok, "client.fetch PUT for alice ok");
  }
  {
    const urlB = `https://_pubky.${B}${P}`;
    const r = await sdk.client.fetch(urlB, {
      method: "PUT",
      body: "bob#3",
      credentials: "include",
    });
    t.ok(r.ok, "client.fetch PUT for bob ok");
  }

  t.equal(await readTextPublic(A, P), "alice#3", "client.fetch wrote to alice");
  t.equal(await readTextPublic(B, P), "bob#3", "client.fetch wrote to bob");

  // 5) Stale-handle safety: Create a third user; ensure earlier Session handles still write correctly.
  const carol = sdk.signer(Keypair.random());
  const carolToken = await createSignupToken();
  const carolSession = await carol.signup(HOMESERVER_PUBLICKEY, carolToken);
  const C = carolSession.info.publicKey.z32();

  await aliceSession.storage.putText(P, "alice#4");
  await bobSession.storage.putText(P, "bob#4");
  t.equal(
    await readTextPublic(A, P),
    "alice#4",
    "stale alice handle still targets alice",
  );
  t.equal(
    await readTextPublic(B, P),
    "bob#4",
    "stale bob handle still targets bob",
  );

  await carolSession.storage.putText(P, "carol#1");
  t.equal(
    await readTextPublic(C, P),
    "carol#1",
    "carol write lands under carol",
  );

  t.end();
});

/**
 * Simulates a user repeatedly signing up and signing out — which in browsers often
 * correlates with page reloads and “switch account” flows.
 *
 * We assert that:
 *  - Each *new* session writes only for its own user.
 *  - The most recently *signed-out* user cannot write anymore (401).
 *  - Older Session handles never “jump” to a newer identity.
 */
test("Auth: signup/signout loops keep cookies and host in sync", async (t) => {
  const sdk = Pubky.testnet();

  const P: Path = "/pub/example.com/loop.txt";

  async function signupAndMark(label: string): Promise<{
    signer: ReturnType<Facade["signer"]>;
    session: SignupSession;
    user: string;
  }> {
    const signer = sdk.signer(Keypair.random());
    const token = await createSignupToken();
    const session = await signer.signup(HOMESERVER_PUBLICKEY, token);
    const user = session.info.publicKey.z32();
    await session.storage.putText(P, label);
    return { signer, session, user };
  }

  const u1 = await signupAndMark("user#1:hello");
  t.equal(
    await sdk.publicStorage.getText(`pubky${u1.user}${P}` as Address),
    "user#1:hello",
    "first user marked",
  );

  await u1.session.signout();

  {
    const url = `https://_pubky.${u1.user}${P}`;
    const r = await sdk.client.fetch(url, {
      method: "PUT",
      body: "should-401",
      credentials: "include",
    });
    t.equal(r.status, 401, "signed-out user cannot write");
  }

  const u2 = await signupAndMark("user#2:hello");
  t.equal(
    await sdk.publicStorage.getText(`pubky${u2.user}${P}` as Address),
    "user#2:hello",
    "second user marked",
  );

  try {
    await u1.session.storage.putText(P, "nope");
    t.fail("stale user#1 session should not be able to write after signout");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "RequestError", "stale handle write -> Error");
  }

  {
    const url = `https://_pubky.${u2.user}${P}`;
    const r = await sdk.client.fetch(url, {
      method: "PUT",
      body: "user#2:via-client",
      credentials: "include",
    });
    t.ok(r.ok, "low-level client PUT for user#2 ok");
  }
  t.equal(
    await sdk.publicStorage.getText(`pubky${u2.user}${P}` as Address),
    "user#2:via-client",
    "low-level client wrote under user#2",
  );

  t.end();
});
