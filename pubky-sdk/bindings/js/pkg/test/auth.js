import test from "tape";
import { PublicKey } from "../index.cjs";
import { createSignupToken } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

// relay base (no trailing slash is fine; the flow will append the channel id)
const TESTNET_HTTP_RELAY = "http://localhost:15412/link";

test("Auth: 3rd party signin", async (t) => {
  const sdk = Pubky.testnet(); // or Pubky.testnet("localhost")

  const signer = sdk.signerRandom();
  const pubky = signer.publicKey().z32();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
  const flow = sdk.startAuthFlow(capabilities, TESTNET_HTTP_RELAY);
  const authUrl = flow.authorizationUrl();

  {
    const signupToken = await createSignupToken();
    await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
    await signer.approveAuthRequest(authUrl);
  }

  const session = await flow.awaitApproval();

  t.equal(session.info().publicKey().z32(), pubky, "session belongs to expected user");
  t.deepEqual(
    session.info().capabilities(),
    capabilities.split(","),
    "session capabilities match",
  );

  t.end();
});

// test("AuthFlow.start: rejects malformed capabilities; normalizes valid; allows empty", async (t) => {
//   useTestnet();

//   // 1) Invalid entries -> throws InvalidInput
//   try {
//     AuthFlow.start("/ok/:rw,not/a/cap,/also:bad:x", TESTNET_HTTP_RELAY);
//     t.fail("start() should throw on malformed capability entries");
//   } catch (e) {
//     t.equal(e.name, "InvalidInput", "invalid caps -> InvalidInput");
//     t.ok(
//       /Invalid capability entries/i.test(e.message),
//       "error message mentions invalid entries"
//     );
//   }

//   // 2) Valid entry with unordered actions -> normalization in URL (wr -> rw)
//   {
//     const flow = AuthFlow.start("/pub/example/:wr", TESTNET_HTTP_RELAY);
//     const url = new URL(flow.authorizationUrl());
//     const caps = url.searchParams.get("caps");
//     t.equal(caps, "/pub/example/:rw", "actions normalized to ':rw' in deep link");
//   }

//   // 3) Empty string -> allowed, caps param stays empty
//   {
//     const flow = AuthFlow.start("", TESTNET_HTTP_RELAY);
//     const url = new URL(flow.authorizationUrl());
//     const caps = url.searchParams.get("caps");
//     t.equal(caps, "", "empty input allowed (no scopes)");
//   }

//   t.end();
// });

// test("validateCapabilities(): ok, normalize, and precise errors", async (t) => {
//   // OK + normalization
//   t.equal(
//     validateCapabilities("/pub/a/:wr,/priv/b/:r"),
//     "/pub/a/:rw,/priv/b/:r",
//     "normalize wr->rw and keep valid entries"
//   );

//   // Precise error message
//   try {
//     validateCapabilities("/pub/a/:rw,/x:y,/pub/b/:x");
//     t.fail("validateCapabilities should throw on malformed entries");
//   } catch (e) {
//     t.equal(e.name, "InvalidInput", "helper throws InvalidInput on bad entries");
//     t.ok(
//       e.message.includes("/x:y") && e.message.includes("/pub/b/:x"),
//       "message lists bad entries"
//     );
//   }

//   t.end();
// });