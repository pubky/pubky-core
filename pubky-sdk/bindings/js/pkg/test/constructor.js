import test from "tape";
import { Client } from "../index.cjs";

test("new Client() without config", async (t) => {
  const client = new Client(); // Should always work
  t.ok(client, "should create a client");
});

test("new Client() with config", async (t) => {
  const client = new Client({
    pkarr: {
      relays: ["http://localhost:15412/relay"],
      requestTimeout: 1000, // ms
    },
  });
  t.ok(client, "should create a client");
});

test("new Client() partial config", async (t) => {
  // Partial pkarr config is fine
  const client = new Client({
    pkarr: {
      relays: ["http://localhost:15412/relay"],
    },
  });
  t.ok(client, "should create a client");
});

test("new Client() with faulty config", async (t) => {
  // Request timeout must be non-zero; 0 should throw (NonZeroU64)
  t.throws(
    () =>
      new Client({
        pkarr: { requestTimeout: 0 },
      }),
    "should throw an error",
  );
});
