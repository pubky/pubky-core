// import test from "tape";
// import { useTestnet, Keypair, PublicKey, Signer, Pkdns } from "../index.cjs";
// import { createSignupToken } from "./utils.js";

// const HOMESERVER_PUBLICKEY = PublicKey.from(
//   "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
// );

// // PKDNS: not found for a fresh key (no record published)
// test("pkdns: getHomeserver not found", async (t) => {
//   useTestnet();

//   const kp = Keypair.random();
//   const pubkey = kp.publicKey();

//   const pkdns = new Pkdns(); // read-only resolver
//   const hs = await pkdns.getHomeserverOf(pubkey);

//   t.equal(hs, undefined, "no homeserver for a fresh keypair");
//   t.end();
// });

// // PKDNS: success after signup (record published during signup)
// test("pkdns: getHomeserver success", async (t) => {
//   useTestnet();

//   const signer = Signer.random();
//   const signupToken = await createSignupToken();
//   await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

//   const pubkey = signer.publicKey();

//   // Read-only resolver
//   const pkdns = new Pkdns();
//   const hs = await pkdns.getHomeserverOf(pubkey);
//   t.equal(hs, HOMESERVER_PUBLICKEY.z32(), "resolver matches homeserver z32");

//   // Self resolver (via signer-bound PKDNS)
//   const selfDns = signer.pkdns();
//   const hsSelf = await selfDns.getHomeserver();
//   t.equal(hsSelf, HOMESERVER_PUBLICKEY.z32(), "self getHomeserver matches");

//   t.end();
// });

// test("pkdns: ifStale is a no-op when fresh; force overrides", async (t) => {
//   useTestnet();

//   // 1) Signup a user so an initial _pubky record exists
//   const signer = Signer.random();
//   const signupToken = await createSignupToken();
//   const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
//   const userPk = session.info().publicKey();

//   const pkdns = signer.pkdns();
//   const readOnlyDns = new Pkdns();

//   const altHost1 = PublicKey.from(
//     "m14ckuxretmbwb3cfuucxa8g3o1yzkxu5dx5b5iowxb1onfn6t4o",
//   );
//   const altHost2 = PublicKey.from(
//     "ci6ss67bc3th6uxbwrkimeo7y3rfgs8m59ce8pt6ts5tn8o63cto",
//   );

//   // Sanity: initial host matches homeserver
//   {
//     const readOnlyDns = new Pkdns();
//     const initialHost = await readOnlyDns.getHomeserverOf(userPk);
//     t.equal(
//       initialHost,
//       HOMESERVER_PUBLICKEY.z32(),
//       "initial homeserver matches signup",
//     );
//   }

//   // 2) ifStale with override should NOT change a fresh record
//   {
//     await pkdns.publishHomeserverIfStale(altHost1); // record is fresh, should be a no-op
//     const host = await readOnlyDns.getHomeserverOf(userPk);
//     t.equal(
//       host,
//       HOMESERVER_PUBLICKEY.z32(),
//       "ifStale did not override fresh record",
//     );
//   }

//   // 3) force should override immediately regardless of age
//   {
//     const altHost2z32 = altHost2.z32();
//     await pkdns.publishHomeserverForce(altHost2); // consumes altHost2
//     const host = await readOnlyDns.getHomeserverOf(userPk);
//     t.equal(host, altHost2z32, "force publish overrides regardless of age");
//   }

//   t.end();
// });
