import test from "tape";
import { Client } from "../index.js";
import type { PubkyError } from "../index.js";
import { hasNetwork } from "./utils.js";

const TLD = "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo";

test("basic fetch", async (t) => {
  if (!(await hasNetwork())) {
    t.comment("No network available; skipping basic fetch test");
    t.end();
    return;
  }

  const client = Client.testnet();
  type ClientInstance = typeof client;
  type FetchResult = Awaited<ReturnType<ClientInstance["fetch"]>>;
  void (null as unknown as FetchResult);

  // ICANN domain
  {
    const response = await client.fetch("https://example.com/");
    t.equal(response.status, 200, "fetch example.com ok");
  }

  // Pubky - requires your testnet to be up
  {
    const response = await client.fetch(`https://${TLD}/`);
    t.equal(response.status, 200, "fetch pubky TLD ok (testnet running)");
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
