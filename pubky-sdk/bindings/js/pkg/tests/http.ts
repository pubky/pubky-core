import test from "tape";
import { Client } from "../index.js";
import type { PubkyError } from "../index.js";
import { hasNetwork } from "./utils.js";

function isLikelyNetworkFetchError(error: unknown): error is Error & PubkyError {
  if (error instanceof TypeError && /fetch failed/i.test(error.message)) {
    return true;
  }

  if (
    typeof error === "object" &&
    error !== null &&
    "name" in error &&
    typeof (error as { name?: unknown }).name === "string" &&
    (error as { name: string }).name === "RequestError"
  ) {
    const message = getErrorMessage(error);
    return /\b(fetch failed|ENETUNREACH|EAI_AGAIN|ENOTFOUND|ECONNREFUSED|EHOSTUNREACH)\b/i.test(
      message,
    );
  }

  return false;
}

function getErrorMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as { message?: unknown }).message === "string"
  ) {
    return (error as { message: string }).message;
  }

  return "";
}

const TLD = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";

test("basic fetch", async (t) => {
  const client = Client.testnet();
  type ClientInstance = typeof client;
  type FetchResult = Awaited<ReturnType<ClientInstance["fetch"]>>;
  void (null as unknown as FetchResult);

  // ICANN domain
  {
    if (await hasNetwork()) {
      let response: Response | undefined;

      try {
        response = await client.fetch("https://example.com/");
      } catch (error) {
        if (isLikelyNetworkFetchError(error)) {
          const message = getErrorMessage(error) || "fetch failed";
          t.comment(
            `Unable to reach example.com (${message}); skipping external fetch assertion`,
          );
        } else {
          throw error;
        }
      }

      if (response) {
        t.equal(response.status, 200, "fetch example.com ok");
      }
    } else {
      t.comment("No external network detected; skipping example.com fetch assertion");
    }
  }

  // Pubky - requires your testnet to be up
  {
    try {
      const response = await client.fetch(`https://${TLD}/`);
      t.equal(response.status, 200, "fetch pubky TLD ok (testnet running)");
    } catch (error) {
      if (isLikelyNetworkFetchError(error)) {
        const message = getErrorMessage(error) || "fetch failed";
        t.fail(
          `Pubky fetch failed (${message}). Ensure \`npm run testnet\` is running before tests.`,
        );
        t.end();
        return;
      }

      throw error;
    }
  }

  t.end();
});

test("fetch failed", async (t) => {
  const client = Client.testnet();

  // ICANN: likely fails
  {
    const response = await client
      .fetch("https://nonexistent.domain/")
      .catch((error: unknown) => error as PubkyError);
    t.equal(
      (response as PubkyError).name,
      "RequestError",
      "ICANN fetch error bubbled to JS",
    );
  }

  // Pubky: invalid TLD -> should fail
  {
    const response = await client
      .fetch("https://1pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ew1/")
      .catch((error: unknown) => error as PubkyError);
    t.equal(
      (response as PubkyError).name,
      "PkarrError",
      "pubky fetch error bubbled to JS",
    );
  }

  t.end();
});
