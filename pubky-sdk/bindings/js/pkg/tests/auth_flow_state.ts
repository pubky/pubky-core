import test from "tape";
import { Pubky, AuthFlowKind } from "../index.js";
import { assertPubkyError } from "./utils.js";

const DEAD_RELAY = "http://127.0.0.1:9/link"; // port 9 is typically closed; yields quick connection refusal

// Ensure a second awaitApproval call returns a ClientStateError instead of panicking the WASM layer.
test("AuthFlow: repeat awaitApproval reports ClientStateError", async (t) => {
  const sdk = Pubky.testnet();
  const flow = sdk.startAuthFlow("", AuthFlowKind.signin() ,DEAD_RELAY);

  // First call is expected to reject because the relay is unreachable.
  await flow.awaitApproval().catch(() => {});

  try {
    await flow.awaitApproval();
    t.fail("calling awaitApproval twice should reject with ClientStateError");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "ClientStateError", "second awaitApproval -> ClientStateError");
    t.ok(
      /already called/i.test(error.message),
      "error message explains the flow was already awaited",
    );
  }

  t.end();
});

// Ensure in-flight polling prevents another awaitApproval from consuming the WASM handle.
test("AuthFlow: awaitApproval blocked while tryPollOnce is in-flight", async (t) => {
  const sdk = Pubky.testnet();
  const flow = sdk.startAuthFlow("", AuthFlowKind.signin(), DEAD_RELAY);

  // Kick off a poll without awaiting it to keep an extra handle alive.
  const pendingPoll = flow.tryPollOnce();

  try {
    await flow.awaitApproval();
    t.fail("concurrent awaitApproval should reject with ClientStateError");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "ClientStateError", "rejects with ClientStateError while in use");
    t.ok(
      /in-flight/i.test(error.message),
      "message explains another call is in-flight",
    );
  }

  // Ensure the pending poll settles to avoid unhandled rejections.
  await pendingPoll.catch(() => {});

  t.end();
});

// Ensure borrow-based calls after completion surface a clear ClientStateError instead of null pointer panics.
test("AuthFlow: tryPollOnce after completion reports ClientStateError", async (t) => {
  const sdk = Pubky.testnet();
  const flow = sdk.startAuthFlow("", AuthFlowKind.signin(), DEAD_RELAY);

  await flow.awaitApproval().catch(() => {});

  try {
    await flow.tryPollOnce();
    t.fail("tryPollOnce after completion should reject with ClientStateError");
  } catch (error) {
    assertPubkyError(t, error);
    t.equal(error.name, "ClientStateError", "polling after completion -> ClientStateError");
    t.ok(
      /already completed/i.test(error.message),
      "error message states the flow already completed",
    );
  }

  t.end();
});
