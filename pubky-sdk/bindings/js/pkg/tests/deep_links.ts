import test from "tape";
import {
  PublicKey,
  SigninDeepLink,
  SignupDeepLink
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

// relay base (no trailing slash is fine; the flow will append the channel id)
const TESTNET_HTTP_RELAY = "http://localhost:15412/link";

test("signin deep link valid", async (t) => {
  let url = "pubkyauth://signin?caps=/pub/pubky.app/:rw&relay=http://localhost:15412/link&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8";
  const deepLink = SigninDeepLink.parse(url);
  t.equal(deepLink.capabilities, "/pub/pubky.app/:rw");
  t.equal(deepLink.baseRelayUrl, TESTNET_HTTP_RELAY);
  t.deepEqual(deepLink.secret, new Uint8Array([146, 169, 220, 120, 67, 32, 172, 212, 12, 255, 24, 180, 234, 132, 23, 140, 13, 220, 36, 117, 255, 69, 9, 176, 212, 22, 58, 36, 77, 91, 177, 239]));

  t.equal(deepLink.toString(), url);

  t.end();
});


test("signup deep link valid", async (t) => {
  let url = "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=http://localhost:15412/link&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&hs=8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo&st=1234567890";
  const deepLink = SignupDeepLink.parse(url);
  t.equal(deepLink.capabilities, "/pub/pubky.app/:rw");
  t.equal(deepLink.baseRelayUrl, TESTNET_HTTP_RELAY);
  t.deepEqual(deepLink.secret, new Uint8Array([146, 169, 220, 120, 67, 32, 172, 212, 12, 255, 24, 180, 234, 132, 23, 140, 13, 220, 36, 117, 255, 69, 9, 176, 212, 22, 58, 36, 77, 91, 177, 239]));
  t.equal(deepLink.homeserver.z32(), HOMESERVER_PUBLICKEY.z32());
  t.equal(deepLink.signupToken, "1234567890");

  t.equal(deepLink.toString(), url);

  t.end();
});

