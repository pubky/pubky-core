import test from "tape";
import { Client } from "../index.js";
type ClientOptions = ConstructorParameters<typeof Client>[0];

test("new Client() without config", async (t) => {
  const client = new Client(); // Should always work
  t.ok(client, "should create a client");
});

test("new Client() with config", async (t) => {
  const config = {
    pkarr: {
      relays: ["http://localhost:15412/relay"],
      requestTimeout: 1000, // ms
    },
  } satisfies NonNullable<ClientOptions>;

  const client = new Client(config);
  t.ok(client, "should create a client");
});

test("new Client() partial config", async (t) => {
  // Partial pkarr config is fine
  const config = {
    pkarr: {
      relays: ["http://localhost:15412/relay"],
    },
  } satisfies NonNullable<ClientOptions>;

  const client = new Client(config);
  t.ok(client, "should create a client");
});

test("new Client() with faulty config", async (t) => {
  // Request timeout must be positive; should throw
  t.throws(
    () =>
      new Client({
        pkarr: { requestTimeout: -1000 },
      }),
    "should throw an error",
  );
});
