import test from "tape";
import {
  Keypair,
  Pubky,
  PublicKey,
  SessionInfo,
  validateCapabilities,
  AuthFlowKind
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
const TESTNET_HTTP_RELAY = "http://localhost:15412/inbox";

test("Auth: 3rd party signup", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const pubky = signer.publicKey.z32();
  const signupToken = await createSignupToken();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
  const flow = sdk.startAuthFlow(capabilities, AuthFlowKind.signup(HOMESERVER_PUBLICKEY, signupToken), TESTNET_HTTP_RELAY);

  type Flow = typeof flow;
  type SessionPromise = ReturnType<Flow["awaitApproval"]>;
  type Session = Awaited<SessionPromise>;

  const _flowUrl: Assert<IsExact<Flow["authorizationUrl"], string>> = true;
  const _sessionInfo: Assert<IsExact<Session["info"], SessionInfo>> = true;

  {

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

test("Auth: 3rd party signin", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const pubky = signer.publicKey.z32();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
  const flow = sdk.startAuthFlow(capabilities, AuthFlowKind.signin(), TESTNET_HTTP_RELAY);

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
    sdk.startAuthFlow("/ok/:rw,not/a/cap,/also:bad:x", AuthFlowKind.signin(), TESTNET_HTTP_RELAY);
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
    const flow = sdk.startAuthFlow("/pub/example/:wr", AuthFlowKind.signin(), TESTNET_HTTP_RELAY);
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
    const flow = sdk.startAuthFlow("", AuthFlowKind.signin(), TESTNET_HTTP_RELAY);
    const url = new URL(flow.authorizationUrl);
    const caps = url.searchParams.get("caps");
    t.equal(caps, "", "empty input allowed (no scopes)");
  }

  t.end();
});

test("Auth: resume signin flow reconnects to same channel", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const pubky = signer.publicKey.z32();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";

  // 1) Start a flow and save the URL (as the app would before a refresh).
  const originalFlow = sdk.startAuthFlow(capabilities, AuthFlowKind.signin(), TESTNET_HTTP_RELAY);
  const savedUrl = originalFlow.authorizationUrl;
  // Explicitly free the WASM handle — simulates page refresh killing WASM memory.
  // JS block scoping does NOT trigger WASM destructors; .free() does.
  originalFlow.free();

  // 2) Signer approves while the original flow is gone (token waits in the relay inbox).
  {
    const signupToken = await createSignupToken();
    await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
    await signer.approveAuthRequest(savedUrl);
  }

  // 3) Resume from the saved URL — reconnects to the same relay channel.
  const resumedFlow = sdk.resumeAuthFlow(savedUrl);

  t.equal(
    resumedFlow.authorizationUrl,
    savedUrl,
    "resumed flow produces the same authorization URL",
  );

  const session = await resumedFlow.awaitApproval();

  t.equal(
    session.info.publicKey.z32(),
    pubky,
    "resumed flow session belongs to expected user",
  );
  t.deepEqual(
    session.info.capabilities,
    capabilities.split(","),
    "resumed flow session capabilities match",
  );

  t.end();
});

test("Auth: resume signup flow preserves signup params and channel", async (t) => {
  const sdk = Pubky.testnet();

  const signer = sdk.signer(Keypair.random());
  const pubky = signer.publicKey.z32();
  const signupToken = await createSignupToken();

  const capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";

  // 1) Start a signup flow and save URL before refresh.
  const originalFlow = sdk.startAuthFlow(
    capabilities,
    AuthFlowKind.signup(HOMESERVER_PUBLICKEY, signupToken),
    TESTNET_HTTP_RELAY,
  );
  const savedUrl = originalFlow.authorizationUrl;

  // Signup-specific params are present in the persisted URL.
  const savedParsed = new URL(savedUrl);
  t.equal(
    savedParsed.searchParams.get("hs"),
    HOMESERVER_PUBLICKEY.z32(),
    "saved URL keeps homeserver parameter",
  );
  t.equal(
    savedParsed.searchParams.get("st"),
    signupToken,
    "saved URL keeps signup token parameter",
  );

  // Simulate page refresh destroying the in-memory flow handle.
  originalFlow.free();

  // 2) Signer approves while original flow is gone.
  await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
  await signer.approveAuthRequest(savedUrl);

  // 3) Resume from persisted URL.
  const resumedFlow = sdk.resumeAuthFlow(savedUrl);

  t.equal(
    resumedFlow.authorizationUrl,
    savedUrl,
    "resumed signup flow produces the same authorization URL",
  );

  const resumedParsed = new URL(resumedFlow.authorizationUrl);
  t.equal(
    resumedParsed.searchParams.get("hs"),
    HOMESERVER_PUBLICKEY.z32(),
    "resumed URL keeps homeserver parameter",
  );
  t.equal(
    resumedParsed.searchParams.get("st"),
    signupToken,
    "resumed URL keeps signup token parameter",
  );

  const session = await resumedFlow.awaitApproval();

  t.equal(
    session.info.publicKey.z32(),
    pubky,
    "resumed signup flow session belongs to expected user",
  );
  t.deepEqual(
    session.info.capabilities,
    capabilities.split(","),
    "resumed signup flow session capabilities match",
  );

  t.end();
});

test("resumeAuthFlow: rejects invalid URL", async (t) => {
  const sdk = Pubky.testnet();

  try {
    sdk.resumeAuthFlow("https://not-a-pubkyauth-url.com");
    t.fail("resumeAuthFlow() should throw on non-pubkyauth URL");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "AuthenticationError", "invalid URL -> AuthenticationError");
    t.ok(
      /Failed to parse/i.test(error.message),
      "error message explains parse failure",
    );
  }

  try {
    sdk.resumeAuthFlow("pubkyauth://secret_export?secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8");
    t.fail("resumeAuthFlow() should reject seed export URLs");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "AuthenticationError", "seed export URL -> AuthenticationError");
    t.ok(
      /Only signin and signup/i.test(error.message),
      "error message explains only auth URLs are valid",
    );
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
