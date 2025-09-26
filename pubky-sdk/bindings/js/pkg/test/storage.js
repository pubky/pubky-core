import test from "tape";

import {
  Client,
  Signer,
  PublicStorage,
  Keypair,
  PublicKey,
  setLogLevel,
  useTestnet,
} from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

test("session: put/get/delete, public: get", async (t) => {
  // 0) Point the SDK at the local testnet (relays + wasm HTTP mapping)
  useTestnet();

  // 1) Signer & signup -> ready session (cookie stored by browser/wasm fetch)
  const signer = Signer.random();
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const userPk = session.publicKey();
  const path = "/pub/example.com/arbitrary";
  const addr = `${userPk.z32()}/pub/example.com/arbitrary`; // addressed for public reads
  const json = { foo: "bar" };

  // 2) Write as the user via SessionStorage (absolute path)
  await session.storage().putJson(path, json);
  // 2.1) Read data as the user via SessionStorage (absolute path)
  {
    const got = await session.storage().getJson(path, json);
    t.deepEqual(got, { foo: "bar" }, "public getJson matches");
  }

  // 3) Read publicly (no auth) via PublicStorage
  const publicStorage = new PublicStorage();
  {
    const got = await publicStorage.getJson(addr);
    t.deepEqual(got, { foo: "bar" }, "public getJson matches");
  }

  // 4) Delete as the user
  await session.storage().delete(path);

  // 5) Public GET should 404 now
  try {
    await publicStorage.getJson(addr);
    t.fail("public getJson after delete should 404");
  } catch (e) {
    // Expect your wasm error mapping (RequestError + { statusCode: 404 })
    t.equal(e.name, "RequestError", "mapped error name");
    t.equal(e.statusCode, 404, "status code 404");
  }

  t.end();
});

// test("not found", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   const publicKey = keypair.publicKey();

//   let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

//   let result = await client.fetch(url);

//   t.is(result.status, 404);
// });

// test("unauthorized", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   const session = await client.session(publicKey);
//   t.ok(session, "signup");

//   await client.signout(publicKey);

//   let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

//   // PUT public data, by authorized client
//   let response = await client.fetch(url, {
//     method: "PUT",
//     body: JSON.stringify({ foo: "bar" }),
//     contentType: "json",
//     credentials: "include",
//   });

//   t.equals(response.status, 401);
// });

// test("forbidden", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   const session = await client.session(publicKey);
//   t.ok(session, "signup");

//   const body = JSON.stringify({ foo: "bar" });

//   let url = `pubky://${publicKey.z32()}/priv/example.com/arbitrary`;

//   // PUT public data, by authorized client
//   let response = await client.fetch(url, {
//     method: "PUT",
//     body: JSON.stringify({ foo: "bar" }),
//     credentials: "include",
//   });

//   t.is(response.status, 403);
//   t.is(
//     await response.text(),
//     "Writing to directories other than '/pub/' is forbidden",
//   );
// });

// test("list", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();
//   const pubky = publicKey.z32();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   let urls = [
//     `pubky://${pubky}/pub/a.wrong/a.txt`,
//     `pubky://${pubky}/pub/example.com/a.txt`,
//     `pubky://${pubky}/pub/example.com/b.txt`,
//     `pubky://${pubky}/pub/example.wrong/a.txt`,
//     `pubky://${pubky}/pub/example.com/c.txt`,
//     `pubky://${pubky}/pub/example.com/d.txt`,
//     `pubky://${pubky}/pub/z.wrong/a.txt`,
//   ];

//   for (let url of urls) {
//     await client.fetch(url, {
//       method: "PUT",
//       body: Buffer.from(""),
//       credentials: "include",
//     });
//   }

//   let url = `pubky://${pubky}/pub/example.com/`;

//   {
//     let list = await client.list(url);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/a.txt`,
//         `pubky://${pubky}/pub/example.com/b.txt`,
//         `pubky://${pubky}/pub/example.com/c.txt`,
//         `pubky://${pubky}/pub/example.com/d.txt`,
//       ],
//       "normal list with no limit or cursor",
//     );
//   }

//   {
//     let list = await client.list(url, null, null, 2);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/a.txt`,
//         `pubky://${pubky}/pub/example.com/b.txt`,
//       ],
//       "normal list with limit but no cursor",
//     );
//   }

//   {
//     let list = await client.list(url, "a.txt", null, 2);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/b.txt`,
//         `pubky://${pubky}/pub/example.com/c.txt`,
//       ],
//       "normal list with limit and a suffix cursor",
//     );
//   }

//   {
//     let list = await client.list(
//       url,
//       `pubky://${pubky}/pub/example.com/a.txt`,
//       null,
//       2,
//     );

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/b.txt`,
//         `pubky://${pubky}/pub/example.com/c.txt`,
//       ],
//       "normal list with limit and a full url cursor",
//     );
//   }

//   {
//     let list = await client.list(url, null, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/d.txt`,
//         `pubky://${pubky}/pub/example.com/c.txt`,
//         `pubky://${pubky}/pub/example.com/b.txt`,
//         `pubky://${pubky}/pub/example.com/a.txt`,
//       ],
//       "reverse list with no limit or cursor",
//     );
//   }

//   {
//     let list = await client.list(url, null, true, 2);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/d.txt`,
//         `pubky://${pubky}/pub/example.com/c.txt`,
//       ],
//       "reverse list with limit but no cursor",
//     );
//   }

//   {
//     let list = await client.list(url, "d.txt", true, 2);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.com/c.txt`,
//         `pubky://${pubky}/pub/example.com/b.txt`,
//       ],
//       "reverse list with limit and a suffix cursor",
//     );
//   }
// });

// test("list shallow", async (t) => {
//   const client = Client.testnet();

//   const keypair = Keypair.random();
//   const publicKey = keypair.publicKey();
//   const pubky = publicKey.z32();

//   const signupToken = await createSignupToken(client);

//   await client.signup(keypair, HOMESERVER_PUBLICKEY, signupToken);

//   let urls = [
//     `pubky://${pubky}/pub/a.com/a.txt`,
//     `pubky://${pubky}/pub/example.com/a.txt`,
//     `pubky://${pubky}/pub/example.com/b.txt`,
//     `pubky://${pubky}/pub/example.com/c.txt`,
//     `pubky://${pubky}/pub/example.com/d.txt`,
//     `pubky://${pubky}/pub/example.con/d.txt`,
//     `pubky://${pubky}/pub/example.con`,
//     `pubky://${pubky}/pub/file`,
//     `pubky://${pubky}/pub/file2`,
//     `pubky://${pubky}/pub/z.com/a.txt`,
//   ];

//   for (let url of urls) {
//     await client.fetch(url, {
//       method: "PUT",
//       body: Buffer.from(""),
//       credentials: "include",
//     });
//   }

//   let url = `pubky://${pubky}/pub/`;

//   {
//     let list = await client.list(url, null, false, null, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/a.com/`,
//         `pubky://${pubky}/pub/example.com/`,
//         `pubky://${pubky}/pub/example.con`,
//         `pubky://${pubky}/pub/example.con/`,
//         `pubky://${pubky}/pub/file`,
//         `pubky://${pubky}/pub/file2`,
//         `pubky://${pubky}/pub/z.com/`,
//       ],
//       "normal list shallow",
//     );
//   }

//   {
//     let list = await client.list(url, null, false, 3, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/a.com/`,
//         `pubky://${pubky}/pub/example.com/`,
//         `pubky://${pubky}/pub/example.con`,
//       ],
//       "normal list shallow with limit",
//     );
//   }

//   {
//     let list = await client.list(url, `example.com/`, false, null, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/example.con`,
//         `pubky://${pubky}/pub/example.con/`,
//         `pubky://${pubky}/pub/file`,
//         `pubky://${pubky}/pub/file2`,
//         `pubky://${pubky}/pub/z.com/`,
//       ],
//       "normal list shallow with cursor",
//     );
//   }

//   {
//     let list = await client.list(url, null, true, null, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/z.com/`,
//         `pubky://${pubky}/pub/file2`,
//         `pubky://${pubky}/pub/file`,
//         `pubky://${pubky}/pub/example.con/`,
//         `pubky://${pubky}/pub/example.con`,
//         `pubky://${pubky}/pub/example.com/`,
//         `pubky://${pubky}/pub/a.com/`,
//       ],
//       "normal list shallow",
//     );
//   }

//   {
//     let list = await client.list(url, null, true, 3, true);

//     t.deepEqual(
//       list,
//       [
//         `pubky://${pubky}/pub/z.com/`,
//         `pubky://${pubky}/pub/file2`,
//         `pubky://${pubky}/pub/file`,
//       ],
//       "normal list shallow with limit",
//     );
//   }
// });
