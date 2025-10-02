import test from "tape";

import { Pubky, PublicKey, Keypair } from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

test("session: putJson/getJson/delete, public: getJson", async (t) => {
  // 0) Use the façade pre-wired for local testnet (PKARR + WASM http mapping)
  const sdk = Pubky.testnet();

  // 1) Signer & signup -> ready session (cookie managed by fetch)
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey;
  const path = "/pub/example.com/arbitrary";
  const addr = `${userPk.z32()}/pub/example.com/arbitrary`;
  const json = { foo: "bar" };

  // 2) Write as the user via SessionStorage (absolute path)
  await session.storage.putJson(path, json);

  // 3) Read data as the user via SessionStorage (absolute path)
  {
    const got = await session.storage.getJson(path);
    t.deepEqual(got, { foo: "bar" }, "session getJson matches");
  }

  // 4) Read publicly (no auth) via PublicStorage
  {
    const got = await sdk.publicStorage.getJson(addr);
    t.deepEqual(got, { foo: "bar" }, "public getJson matches");
  }

  // 5) Delete as the user
  await session.storage.delete(path);

  // 6) Public GET should 404 now
  try {
    await sdk.publicStorage.getJson(addr);
    t.fail("public getJson after delete should 404");
  } catch (e) {
    t.equal(e.name, "RequestError", "mapped error name");
    t.equal(e.statusCode, 404, "status code 404");
  }

  t.end();
});

test("session: putText/getText/delete, public: getText", async (t) => {
  const sdk = Pubky.testnet();

  // 1) signer -> signup -> session
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = "/pub/example.com/hello.txt"; // session-scoped absolute path
  const addr = `${userPk}/pub/example.com/hello.txt`; // addressed for public reads
  const text = "hello world from pubky";

  // 2) write text as the user
  await session.storage.putText(path, text);

  // 3) read text back via session
  {
    const got = await session.storage.getText(path);
    t.equal(got, text, "session getText matches");
  }

  // 4) read text publicly (no auth)
  {
    const got = await sdk.publicStorage.getText(addr);
    t.equal(got, text, "public getText matches");
  }

  // 5) delete
  await session.storage.delete(path);

  // 6) public GET should 404
  {
    try {
      await sdk.publicStorage.getText(addr);
      t.fail("public getText after delete should 404");
    } catch (e) {
      t.equal(e.name, "RequestError", "mapped error name");
      t.equal(e.statusCode, 404, "status code 404");
    }
  }

  t.end();
});

test("session: putBytes/getBytes/delete, public: getBytes", async (t) => {
  const sdk = Pubky.testnet();

  // 1) signer -> signup -> session
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = "/pub/example.com/blob.bin"; // session-scoped absolute path
  const addr = `${userPk}/pub/example.com/blob.bin`; // addressed for public reads

  // Bytes payload (Buffer is a Uint8Array in Node)
  const bytes = Buffer.from([0, 1, 2, 3, 4, 250, 251, 252, 253, 254, 255]);

  // 2) write bytes
  await session.storage.putBytes(path, bytes);

  // 3) read bytes back via session
  {
    const got = await session.storage.getBytes(path); // Uint8Array
    t.deepEqual([...got], [...bytes], "session getBytes matches");
  }

  // 4) read bytes publicly
  {
    const got = await sdk.publicStorage.getBytes(addr); // Uint8Array
    t.deepEqual([...got], [...bytes], "public getBytes matches");
  }

  // 5) delete
  await session.storage.delete(path);

  // 6) public GET should 404
  {
    try {
      await sdk.publicStorage.getBytes(addr);
      t.fail("public getBytes after delete should 404");
    } catch (e) {
      t.equal(e.name, "RequestError", "mapped error name");
      t.equal(e.statusCode, 404, "status code 404");
    }
  }

  t.end();
});

// Missing resource: exists=false; getJson throws 404.
test("not found", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const addr = `${userPk}/pub/example.com/definitely-missing.json`;

  t.equal(
    await sdk.publicStorage.exists(addr),
    false,
    "exists() is false on missing path",
  );

  try {
    await sdk.publicStorage.getJson(addr);
    t.fail("getJson() should throw on missing");
  } catch (e) {
    t.equal(e.name, "RequestError", "mapped error name");
    t.equal(e.statusCode, 404, "status code 404");
  }

  t.end();
});

// Unauthorized write after signout must return 401.
test("unauthorized (no cookie) PUT returns 401", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const url = `pubky://${userPk}/pub/example.com/unauth.json`;

  await session.signout();

  const resp = await sdk.client.fetch(url, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ foo: "bar" }),
    credentials: "include",
  });

  t.equal(resp.status, 401, "PUT without valid session cookie is 401");
  t.end();
});

test("forbidden: writing outside /pub returns 403", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const path = "/priv/example.com/arbitrary";
  try {
    await session.storage.putText(path, "Hello");
    t.fail("putText to /priv should fail with 403");
  } catch (e) {
    t.equal(e.name, "RequestError", "mapped error name");
    t.equal(e.statusCode, 403, "status code 403");
    t.ok(
      String(e.message || "").includes(
        "Writing to directories other than '/pub/'",
      ),
      "error message mentions /pub restriction",
    );
  }

  t.end();
});

test("list (public dir listing with limit/cursor/reverse)", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();

  // Create files. Only the ones under /pub/example.com/ should show in listing.
  const mk = (p) => session.storage.putText(p, "");
  await mk(`/pub/a.wrong/a.txt`);
  await mk(`/pub/example.com/a.txt`);
  await mk(`/pub/example.com/b.txt`);
  await mk(`/pub/example.wrong/a.txt`);
  await mk(`/pub/example.com/c.txt`);
  await mk(`/pub/example.com/d.txt`);
  await mk(`/pub/z.wrong/a.txt`);

  const dir = `${userPk}/pub/example.com/`; // addressed dir path (must end with '/')

  // 1) normal list (no limit/cursor), forward
  {
    const list = await sdk.publicStorage.list(dir);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/a.txt`,
        `pubky://${userPk}/pub/example.com/b.txt`,
        `pubky://${userPk}/pub/example.com/c.txt`,
        `pubky://${userPk}/pub/example.com/d.txt`,
      ],
      "normal list with no limit or cursor",
    );
  }

  // 2) limit=2 (forward)
  {
    const list = await sdk.publicStorage.list(dir, null, false, 2);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/a.txt`,
        `pubky://${userPk}/pub/example.com/b.txt`,
      ],
      "forward list with limit but no cursor",
    );
  }

  // 3) cursor suffix "a.txt", limit=2 (forward)
  {
    const list = await sdk.publicStorage.list(dir, "a.txt", false, 2);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/b.txt`,
        `pubky://${userPk}/pub/example.com/c.txt`,
      ],
      "forward list with limit and a suffix cursor",
    );
  }

  // 4) cursor as full URL, limit=2 (forward)
  {
    const list = await sdk.publicStorage.list(
      dir,
      `pubky://${userPk}/pub/example.com/a.txt`,
      false,
      2,
    );
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/b.txt`,
        `pubky://${userPk}/pub/example.com/c.txt`,
      ],
      "forward list with limit and a full url cursor",
    );
  }

  // 5) reverse listing (no limit)
  {
    const list = await sdk.publicStorage.list(dir, null, true);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/d.txt`,
        `pubky://${userPk}/pub/example.com/c.txt`,
        `pubky://${userPk}/pub/example.com/b.txt`,
        `pubky://${userPk}/pub/example.com/a.txt`,
      ],
      "reverse list with no limit or cursor",
    );
  }

  // 6) reverse + limit=2
  {
    const list = await sdk.publicStorage.list(dir, null, true, 2);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/d.txt`,
        `pubky://${userPk}/pub/example.com/c.txt`,
      ],
      "reverse list with limit but no cursor",
    );
  }

  // 7) reverse + suffix cursor "d.txt" + limit=2
  {
    const list = await sdk.publicStorage.list(dir, "d.txt", true, 2);
    t.deepEqual(
      list,
      [
        `pubky://${userPk}/pub/example.com/c.txt`,
        `pubky://${userPk}/pub/example.com/b.txt`,
      ],
      "reverse list with limit and a suffix cursor",
    );
  }

  t.end();
});

test("list shallow under /pub/", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const pubky = session.info.publicKey.z32();
  const put = (p) => session.storage.putBytes(p, new Uint8Array());

  // Seed files (directories appear because they contain files; also create same-stem file+dir case)
  await Promise.all([
    put("/pub/a.com/a.txt"),
    put("/pub/example.com/a.txt"),
    put("/pub/example.com/b.txt"),
    put("/pub/example.com/c.txt"),
    put("/pub/example.com/d.txt"),
    put("/pub/example.con/d.txt"), // creates /pub/example.con/ as a directory
    put("/pub/example.con"), // also a *file* with same stem (no trailing slash)
    put("/pub/file"),
    put("/pub/file2"),
    put("/pub/z.com/a.txt"),
  ]);

  const dirPath = "/pub/";
  // const dirAddr = `${pubky}/pub/`; // addressed public listing (optional parity checks)

  // 1) shallow list (session, forward, no limit)
  {
    const list = await session
      .storage
      .list(dirPath, undefined, false, undefined, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/a.com/`,
        `pubky://${pubky}/pub/example.com/`,
        `pubky://${pubky}/pub/example.con`,
        `pubky://${pubky}/pub/example.con/`,
        `pubky://${pubky}/pub/file`,
        `pubky://${pubky}/pub/file2`,
        `pubky://${pubky}/pub/z.com/`,
      ],
      "shallow forward list with no limit",
    );
  }

  // 2) shallow list with limit=3 (session, forward)
  {
    const list = await session
      .storage
      .list(dirPath, undefined, false, 3, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/a.com/`,
        `pubky://${pubky}/pub/example.com/`,
        `pubky://${pubky}/pub/example.con`,
      ],
      "shallow forward list with limit",
    );
  }

  // 3) shallow list with suffix cursor (session, forward)
  {
    const list = await session
      .storage
      .list(dirPath, "example.com/", false, undefined, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.con`,
        `pubky://${pubky}/pub/example.con/`,
        `pubky://${pubky}/pub/file`,
        `pubky://${pubky}/pub/file2`,
        `pubky://${pubky}/pub/z.com/`,
      ],
      "shallow forward list with suffix cursor",
    );
  }

  // 4) shallow reverse list (session, no limit)
  {
    const list = await session
      .storage
      .list(dirPath, undefined, true, undefined, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/z.com/`,
        `pubky://${pubky}/pub/file2`,
        `pubky://${pubky}/pub/file`,
        `pubky://${pubky}/pub/example.con/`,
        `pubky://${pubky}/pub/example.con`,
        `pubky://${pubky}/pub/example.com/`,
        `pubky://${pubky}/pub/a.com/`,
      ],
      "shallow reverse list with no limit",
    );
  }

  // 5) shallow reverse with limit=3 (session)
  {
    const list = await session
      .storage
      .list(dirPath, undefined, true, 3, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/z.com/`,
        `pubky://${pubky}/pub/file2`,
        `pubky://${pubky}/pub/file`,
      ],
      "shallow reverse list with limit",
    );
  }

  t.end();
});

/**
 * stats()/exists() for JSON content.
 * - Write JSON via SessionStorage.
 * - Check exists() in both session and public modes.
 * - stats() must be non-null and consistent across session/public.
 * - lastModifiedMs increases after an update (with a short delay for clock resolution).
 * - contentLength must equal the actual stored bytes length.
 * - etag (if present) should change after content update.
 */
test("stats & exists: JSON (session + public)", async (t) => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const sdk = Pubky.testnet();

  // 1) signup -> session
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = "/pub/example.com/meta.json"; // session-scoped
  const addr = `${userPk}/pub/example.com/meta.json`; // public addressed
  const payload1 = { hello: "world" };
  const payload2 = { hello: "pubky", n: 2 };

  // 2) write JSON
  await session.storage.putJson(path, payload1);

  // 3) exists(): session & public
  t.equal(
    await session.storage.exists(path),
    true,
    "session.exists() -> true",
  );
  t.equal(
    await sdk.publicStorage.exists(addr),
    true,
    "public.exists() -> true",
  );

  // 4) stats(): session & public (should both be non-null)
  const s1 = await session.storage.stats(path);
  const p1 = await sdk.publicStorage.stats(addr);
  t.ok(s1, "session.stats() not undefined");
  t.ok(p1, "public.stats() not undefined");

  // 5) contentLength equals actual stored bytes length
  {
    const bytes = await sdk.publicStorage.getBytes(addr);
    t.equal(
      p1.contentLength,
      bytes.byteLength,
      "public.stats().contentLength matches actual bytes length",
    );
  }

  // 6) contentType should identify JSON (exact value may vary by server)
  if (p1.contentType != undefined) {
    t.ok(
      /json/i.test(p1.contentType),
      `contentType hints JSON (got: ${p1.contentType})`,
    );
  }

  // 7) lastModifiedMs is a finite number
  t.ok(
    Number.isFinite(p1.lastModifiedMs ?? NaN),
    "lastModifiedMs is a finite number",
  );

  // 8) Update content and observe monotonic lastModifiedMs (+ optional ETag change)
  await sleep(1100); // leave room for mtime resolution
  await session.storage.putJson(path, payload2);

  const p2 = await sdk.publicStorage.stats(addr);
  t.ok(
    p2 && p2.lastModifiedMs > p1.lastModifiedMs,
    "lastModifiedMs increased after update",
  );

  if (p1.etag && p2?.etag) {
    t.notEqual(p2.etag, p1.etag, "etag changed after update (when present)");
  }

  t.end();
});

/**
 * stats()/exists() for missing content.
 * - Ensure exists() is false.
 * - stats() returns undefined (both public and session).
 */
test("stats & exists: missing resource", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = "/pub/example.com/definitely-missing.bin"; // session-scoped
  const addr = `${userPk}/pub/example.com/definitely-missing.bin`; // public addressed

  t.equal(
    await session.storage.exists(path),
    false,
    "session.exists() -> false",
  );
  t.equal(
    await sdk.publicStorage.exists(addr),
    false,
    "public.exists() -> false",
  );

  t.equal(
    await session.storage.stats(path),
    undefined,
    "session.stats() -> undefined",
  );
  t.equal(
    await sdk.publicStorage.stats(addr),
    undefined,
    "public.stats() -> undefined",
  );

  t.end();
});
