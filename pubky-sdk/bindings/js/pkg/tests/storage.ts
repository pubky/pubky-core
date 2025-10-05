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
  assertErrorLike,
  createSignupToken,
  sleep,
} from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

type Facade = ReturnType<typeof Pubky.testnet>;
type Signer = ReturnType<Facade["signer"]>;
type SessionType = Awaited<ReturnType<Signer["signup"]>>;
type SessionStorageType = SessionType["storage"];
type PublicStorageType = Facade["publicStorage"];

type _StorageDelete = Assert<
  IsExact<Parameters<SessionStorageType["delete"]>, [Path]>
>;

const toAddress = (user: string, relPath: Path): Address =>
  `${user}${relPath}` as Address;

test("session: putJson/getJson/delete, public: getJson", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey;
  const path: Path = "/pub/example.com/arbitrary";
  const addr = toAddress(userPk.z32(), path);
  const json = { foo: "bar" };

  await session.storage.putJson(path, json);

  {
    const got = await session.storage.getJson(path);
    t.deepEqual(got, { foo: "bar" }, "session getJson matches");
  }

  {
    const got = await sdk.publicStorage.getJson(addr);
    t.deepEqual(got, { foo: "bar" }, "public getJson matches");
  }

  await session.storage.delete(path);

  try {
    await sdk.publicStorage.getJson(addr);
    t.fail("public getJson after delete should 404");
  } catch (error) {
    assertErrorLike(t, error);
    t.equal(error.name, "RequestError", "mapped error name");
    t.equal(error.statusCode, 404, "status code 404");
  }

  t.end();
});

test("session: putText/getText/delete, public: getText", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path: Path = "/pub/example.com/hello.txt";
  const addr = toAddress(userPk, path);
  const text = "hello world from pubky";

  await session.storage.putText(path, text);

  {
    const got = await session.storage.getText(path);
    t.equal(got, text, "session getText matches");
  }

  {
    const got = await sdk.publicStorage.getText(addr);
    t.equal(got, text, "public getText matches");
  }

  await session.storage.delete(path);

  try {
    await sdk.publicStorage.getText(addr);
    t.fail("public getText after delete should 404");
  } catch (error) {
    assertErrorLike(t, error);
    t.equal(error.name, "RequestError", "mapped error name");
    t.equal(error.statusCode, 404, "status code 404");
  }

  t.end();
});

test("session: putBytes/getBytes/delete, public: getBytes", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path: Path = "/pub/example.com/blob.bin";
  const addr = toAddress(userPk, path);

  const bytes = Uint8Array.from([0, 1, 2, 3, 4, 250, 251, 252, 253, 254, 255]);

  await session.storage.putBytes(path, bytes);

  {
    const got = await session.storage.getBytes(path);
    t.deepEqual([...got], [...bytes], "session getBytes matches");
  }

  {
    const got = await sdk.publicStorage.getBytes(addr);
    t.deepEqual([...got], [...bytes], "public getBytes matches");
  }

  await session.storage.delete(path);

  try {
    await sdk.publicStorage.getBytes(addr);
    t.fail("public getBytes after delete should 404");
  } catch (error) {
    assertErrorLike(t, error);
    t.equal(error.name, "RequestError", "mapped error name");
    t.equal(error.statusCode, 404, "status code 404");
  }

  t.end();
});

test("forbidden: writing outside /pub returns 403", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const forbiddenPath = "/priv/example.com/arbitrary";
  try {
    await session.storage.putText(forbiddenPath as unknown as Path, "Hello");
    t.fail("putText to /priv should fail with 403");
  } catch (error) {
    assertErrorLike(t, error);
    t.equal(error.name, "RequestError", "mapped error name");
    t.equal(error.statusCode, 403, "status code 403");
    t.ok(
      String(error.message || "").includes(
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

  const mk = (p: Path) => session.storage.putText(p, "");
  await mk(`/pub/a.wrong/a.txt` as Path);
  await mk(`/pub/example.com/a.txt` as Path);
  await mk(`/pub/example.com/b.txt` as Path);
  await mk(`/pub/example.wrong/a.txt` as Path);
  await mk(`/pub/example.com/c.txt` as Path);
  await mk(`/pub/example.com/d.txt` as Path);
  await mk(`/pub/example.wrong/d.txt` as Path);
  await mk(`/pub/z.wrong/a.txt` as Path);

  const dir: Address = `${userPk}/pub/example.com/` as Address;

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
  const put = (p: Path) => session.storage.putBytes(p, new Uint8Array());

  await Promise.all([
    put("/pub/a.com/a.txt" as Path),
    put("/pub/example.com/a.txt" as Path),
    put("/pub/example.com/b.txt" as Path),
    put("/pub/example.com/c.txt" as Path),
    put("/pub/example.com/d.txt" as Path),
    put("/pub/example.con/d.txt" as Path),
    put("/pub/example.con" as Path),
    put("/pub/file" as Path),
    put("/pub/file2" as Path),
    put("/pub/z.com/a.txt" as Path),
  ]);

  const dirPath: Path = "/pub/";

  {
    const list = await session.storage.list(
      dirPath,
      undefined,
      false,
      undefined,
      true,
    );
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

  {
    const list = await session.storage.list(dirPath, undefined, false, 3, true);
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
  {
    const list = await session.storage.list(
      dirPath,
      "example.con",
      false,
      undefined,
      true,
    );
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.con/`,
        `pubky://${pubky}/pub/file`,
        `pubky://${pubky}/pub/file2`,
        `pubky://${pubky}/pub/z.com/`,
      ],
      "shallow forward list with suffix cursor",
    );
  }

  {
    const list = await session.storage.list(
      dirPath,
      undefined,
      true,
      undefined,
      true,
    );
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

  {
    const list = await session.storage.list(dirPath, "file2", true, 3, true);
    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/file`,
        `pubky://${pubky}/pub/example.con/`,
        `pubky://${pubky}/pub/example.con`,
      ],
      "shallow reverse list with limit and cursor",
    );
  }

  t.end();
});

test("not found", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const addr = `${userPk}/pub/example.com/definitely-missing.json` as Address;

  t.equal(
    await sdk.publicStorage.exists(addr),
    false,
    "exists() is false on missing path",
  );

  try {
    await sdk.publicStorage.getJson(addr);
    t.fail("getJson() should throw on missing");
  } catch (error) {
    assertErrorLike(t, error);
    t.equal(error.name, "RequestError", "mapped error name");
    t.equal(error.statusCode, 404, "status code 404");
  }

  t.end();
});

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

test("stats & exists: JSON (session + public)", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = `/pub/example.com/stats-${Date.now()}.json` as Path;
  const addr = toAddress(userPk, path);

  const initialJson = { foo: "bar" } as const;
  await session.storage.putJson(path, initialJson);

  t.equal(
    await session.storage.exists(path),
    true,
    "session exists returns true after put",
  );
  t.equal(
    await sdk.publicStorage.exists(addr),
    true,
    "public exists returns true after put",
  );

  const [sessionStats1, publicStats1] = await Promise.all([
    session.storage.stats(path),
    sdk.publicStorage.stats(addr),
  ]);

  t.ok(sessionStats1, "session stats returns metadata");
  t.ok(publicStats1, "public stats returns metadata");

  if (!sessionStats1 || !publicStats1) {
    t.end();
    return;
  }

  const publicBytes = await sdk.publicStorage.getBytes(addr);
  t.equal(
    publicStats1.contentLength,
    publicBytes.byteLength,
    "public contentLength matches getBytes byte length",
  );

  if (publicStats1.contentType !== undefined) {
    t.ok(
      publicStats1.contentType.toLowerCase().includes("json"),
      "public contentType indicates JSON",
    );
  }

  t.ok(
    typeof sessionStats1.lastModifiedMs === "number" &&
      Number.isFinite(sessionStats1.lastModifiedMs),
    "session lastModifiedMs is finite",
  );
  t.ok(
    typeof publicStats1.lastModifiedMs === "number" &&
      Number.isFinite(publicStats1.lastModifiedMs),
    "public lastModifiedMs is finite",
  );

  await sleep(1100);

  const updatedJson = { foo: "baz" } as const;
  await session.storage.putJson(path, updatedJson);

  const [sessionStats2, publicStats2] = await Promise.all([
    session.storage.stats(path),
    sdk.publicStorage.stats(addr),
  ]);

  t.ok(sessionStats2, "session stats returns metadata after update");
  t.ok(publicStats2, "public stats returns metadata after update");

  if (!sessionStats2 || !publicStats2) {
    t.end();
    return;
  }

  if (
    typeof sessionStats1.lastModifiedMs === "number" &&
    typeof sessionStats2.lastModifiedMs === "number"
  ) {
    t.ok(
      sessionStats2.lastModifiedMs > sessionStats1.lastModifiedMs,
      "session lastModifiedMs increases after update",
    );
  }

  if (
    typeof publicStats1.lastModifiedMs === "number" &&
    typeof publicStats2.lastModifiedMs === "number"
  ) {
    t.ok(
      publicStats2.lastModifiedMs > publicStats1.lastModifiedMs,
      "public lastModifiedMs increases after update",
    );
  }

  if (sessionStats1.etag && sessionStats2.etag) {
    t.notEqual(
      sessionStats2.etag,
      sessionStats1.etag,
      "session etag changes after update",
    );
  }

  if (publicStats1.etag && publicStats2.etag) {
    t.notEqual(
      publicStats2.etag,
      publicStats1.etag,
      "public etag changes after update",
    );
  }

  t.end();
});

test("stats & exists: missing resource", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.info.publicKey.z32();
  const path = `/pub/example.com/missing-${Date.now()}.json` as Path;
  const addr = toAddress(userPk, path);

  t.equal(
    await session.storage.exists(path),
    false,
    "session exists returns false for missing path",
  );
  t.equal(
    await sdk.publicStorage.exists(addr),
    false,
    "public exists returns false for missing path",
  );

  const [sessionStats, publicStats] = await Promise.all([
    session.storage.stats(path),
    sdk.publicStorage.stats(addr),
  ]);

  t.equal(
    sessionStats,
    undefined,
    "session stats is undefined for missing path",
  );
  t.equal(publicStats, undefined, "public stats is undefined for missing path");

  t.end();
});
