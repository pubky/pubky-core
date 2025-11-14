import test from "tape";
import {
  Keypair,
  Pubky,
  PublicKey,
  SessionInfo,
  validateCapabilities,
} from "../index.js";
import {
  Assert,
  IsExact,
  assertPubkyError,
  createSignupToken,
  TESTNET_HTTP_RELAY,
} from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

test("Auth: 3rd party signin", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const pubky = signer.publicKey.z32();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
  const flow = sdk.startAuthFlow(capabilities, TESTNET_HTTP_RELAY);

  type Flow = typeof flow;
  type SessionPromise = ReturnType<Flow["awaitApproval"]>;
  type Session = Awaited<SessionPromise>;

  const _flowUrl: Assert<IsExact<Flow["authorizationUrl"], string>> = true;
  const _sessionInfo: Assert<IsExact<Session["info"], SessionInfo>> = true;

  {
    const signupToken = await createSignupToken();
    await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
    await signer.approveAuthRequest(flow.authorizationUrl);
  }

  const session = await flow.awaitApproval();

  t.equal(
    session.info.publicKey.z32(),
    pubky,
    "session belongs to expected user",
  );
  t.deepEqual(
    session.info.capabilities,
    capabilities.split(","),
    "session capabilities match",
  );

  t.end();
});

test("startAuthFlow: rejects malformed capabilities; normalizes valid; allows empty", async (t) => {
  const sdk = Pubky.testnet(); // uses local testnet mapping so URLs are resolvable in-node

  // 1) Invalid entries -> throws InvalidInput with a precise message
  try {
    // @ts-ignore: invalid capabilities string format. Emulating plain JS validation rules.
    sdk.startAuthFlow("/ok/:rw,not/a/cap,/also:bad:x", TESTNET_HTTP_RELAY);
    t.fail("startAuthFlow() should throw on malformed capability entries");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "InvalidInput", "invalid caps -> InvalidInput");
    t.ok(
      /Invalid capability entries/i.test(error.message),
      "error message lists invalid entries",
    );
    t.ok(
      error.message.includes("not/a/cap") &&
        error.message.includes("/also:bad:x"),
      "message includes concrete bad entries",
    );
    t.ok(
      error.data &&
        typeof error.data === "object" &&
        Array.isArray((error.data as { invalidEntries?: unknown }).invalidEntries),
      "error.data exposes invalidEntries array",
    );
    if (
      error.data &&
      typeof error.data === "object" &&
      Array.isArray((error.data as { invalidEntries?: unknown }).invalidEntries)
    ) {
      t.deepEqual(
        (error.data as { invalidEntries: string[] }).invalidEntries,
        ["not/a/cap", "/also:bad:x"],
        "invalidEntries matches malformed tokens",
      );
    }
  }

  // 2) Valid entry with unordered actions -> normalized in URL (wr -> rw)
  {
    // @ts-ignore: invalid capabilities string format. Emulating plain JS normalization.
    const flow = sdk.startAuthFlow("/pub/example/:wr", TESTNET_HTTP_RELAY);
    const url = new URL(flow.authorizationUrl);
    const caps = url.searchParams.get("caps");
    t.equal(
      caps,
      "/pub/example/:rw",
      "actions normalized to ':rw' in deep link",
    );
  }

  // 3) Empty string -> allowed; caps param remains empty
  {
    const flow = sdk.startAuthFlow("", TESTNET_HTTP_RELAY);
    const url = new URL(flow.authorizationUrl);
    const caps = url.searchParams.get("caps");
    t.equal(caps, "", "empty input allowed (no scopes)");
  }

  t.end();
});

// Covers the pure string validator without running the flow.
// Ensures normalization behavior and precise error reporting.
test("validateCapabilities(): ok, normalize, and precise errors", async (t) => {
  // OK + normalization
  t.equal(
    // @ts-ignore: invalid capabilities string format. Emulating plain JS validation rules.
    validateCapabilities("/pub/a/:wr,/priv/b/:r"),
    "/pub/a/:rw,/priv/b/:r",
    "normalize wr->rw and preserve valid entries",
  );

  // Precise error message for malformed entries
  try {
    // @ts-ignore: invalid capabilities string format. Emulating plain JS validation rules.
    validateCapabilities("/pub/a/:rw,/x:y,/pub/b/:x");
    t.fail("validateCapabilities should throw on malformed entries");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "InvalidInput", "throws InvalidInput on bad entries");
    t.ok(
      error.message.includes("/x:y") && error.message.includes("/pub/b/:x"),
      "message lists all offending entries",
    );
    t.ok(
      error.data &&
        typeof error.data === "object" &&
        Array.isArray((error.data as { invalidEntries?: unknown }).invalidEntries),
      "error.data exposes invalidEntries array",
    );
    if (
      error.data &&
      typeof error.data === "object" &&
      Array.isArray((error.data as { invalidEntries?: unknown }).invalidEntries)
    ) {
      t.deepEqual(
        (error.data as { invalidEntries: string[] }).invalidEntries,
        ["/x:y", "/pub/b/:x"],
        "invalidEntries matches malformed tokens",
      );
    }
  }

  t.end();
});
