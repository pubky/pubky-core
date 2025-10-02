import test from "tape";
import { Pubky, Keypair, PublicKey } from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

/**
 * PKDNS: fresh key has no _pubky record.
 * Flow:
 *  - façade -> read-only pkdns resolver
 *  - generate keypair without publishing any record
 *  - resolver returns undefined
 */
test("pkdns: getHomeserver not found", async (t) => {
  const sdk = Pubky.testnet();

  const fresh = Keypair.random();
  const pubkey = fresh.publicKey;

  const hs = await sdk.pkdns.getHomeserverOf(pubkey);

  t.equal(hs, undefined, "no homeserver for a fresh keypair");
  t.end();
});

/**
 * PKDNS: signup publishes _pubky; both read-only and signer-bound resolvers agree.
 * Flow:
 *  - façade -> signer -> signup -> server publishes _pubky(host=homeserver)
 *  - read-only resolver returns homeserver z32
 *  - signer-bound resolver returns the same
 */
test("pkdns: getHomeserver success", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  await signer.signup(HOMESERVER_PUBLICKEY, signupToken);

  const pubkey = signer.publicKey;

  // Read-only resolver
  const hs = await sdk.pkdns.getHomeserverOf(pubkey);
  t.equal(hs, HOMESERVER_PUBLICKEY.z32(), "resolver matches homeserver z32");

  // Self resolver (signer-bound)
  const hsSelf = await signer.pkdns.getHomeserver();
  t.equal(hsSelf, HOMESERVER_PUBLICKEY.z32(), "self getHomeserver matches");

  t.end();
});

/**
 * PKDNS: IfStale respects freshness; Force overrides immediately.
 * Flow:
 *  - signup publishes initial record (fresh)
 *  - publishHomeserverIfStale(alt) is a no-op when record is fresh
 *  - publishHomeserverForce(alt2) overrides regardless of age
 */
test("pkdns: ifStale is a no-op when fresh; force overrides", async (t) => {
  const sdk = Pubky.testnet();

  // 1) Signup a user so an initial _pubky record exists
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
  const userPk = session.info.publicKey;

  const publisherPkdns = signer.pkdns;
  const readOnlyPkdns = sdk.pkdns;

  const altHost1 = PublicKey.from(
    "m14ckuxretmbwb3cfuucxa8g3o1yzkxu5dx5b5iowxb1onfn6t4o",
  );
  const altHost2 = PublicKey.from(
    "ci6ss67bc3th6uxbwrkimeo7y3rfgs8m59ce8pt6ts5tn8o63cto",
  );

  // Sanity: initial host matches homeserver
  {
    const initialHost = await readOnlyPkdns.getHomeserverOf(userPk);
    t.equal(
      initialHost,
      HOMESERVER_PUBLICKEY.z32(),
      "initial homeserver matches signup",
    );
  }

  // 2) IfStale with override should NOT change a fresh record
  {
    await publisherPkdns.publishHomeserverIfStale(altHost1); // fresh -> no-op
    const host = await readOnlyPkdns.getHomeserverOf(userPk);
    t.equal(
      host,
      HOMESERVER_PUBLICKEY.z32(),
      "ifStale did not override fresh record",
    );
  }

  // 3) Force should override immediately regardless of age
  {
    const altHost2z32 = altHost2.z32();
    await publisherPkdns.publishHomeserverForce(altHost2);
    const host = await readOnlyPkdns.getHomeserverOf(userPk);
    t.equal(host, altHost2z32, "force publish overrides regardless of age");
  }

  t.end();
});
