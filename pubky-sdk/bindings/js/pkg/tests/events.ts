import test from "tape";

import { Keypair, Pubky, PublicKey, type Path } from "../index.js";
import { assertPubkyError, createSignupToken, sleep } from "./utils.js";

const HOMESERVER_PUBLICKEY = PublicKey.from(
  "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo",
);

test("eventStream: comprehensive", async (t) => {
  const sdk = Pubky.testnet();

  // === SETUP: Create ONE user with diverse events ===
  const signer = sdk.signer(Keypair.random());
  const signupToken = await createSignupToken();
  const session = await signer.signup(HOMESERVER_PUBLICKEY, signupToken);
  const userPk = session.info.publicKey;

  // Create 15 files in /pub/app1/
  for (let i = 0; i < 15; i++) {
    const path = `/pub/app1/file_${i}.txt` as Path;
    await session.storage.putText(path, `content ${i}`);
  }

  // Create 10 files in /pub/app2/
  for (let i = 0; i < 10; i++) {
    const path = `/pub/app2/file_${i}.txt` as Path;
    await session.storage.putText(path, `content ${i}`);
  }

  // Create 3 files in /pub/photos/
  for (let i = 0; i < 3; i++) {
    const path = `/pub/photos/pic_${i}.jpg` as Path;
    await session.storage.putText(path, `photo ${i}`);
  }

  // Add some DELETE events (delete 3 files from app1)
  for (let i = 12; i < 15; i++) {
    const path = `/pub/app1/file_${i}.txt` as Path;
    await session.storage.delete(path);
  }

  await sleep(500); // Wait for events to be recorded

  // Total events: 28 PUT + 3 DEL = 31 events

  // === Test 1: Historical events with limit ===
  t.comment("Test 1: Historical events with limit");

  const stream1 = await sdk.eventStream().addUser(userPk, null).limit(10).subscribe();

  const events1 = [];
  const reader1 = stream1.getReader();

  try {
    while (true) {
      const { done, value } = await reader1.read();
      if (done) break;
      events1.push(value);
    }

    t.equal(events1.length, 10, "should receive exactly 10 events");

    for (const event of events1) {
      t.equal(typeof event.eventType, "string", "event type should be string");
      t.ok(event.resource, "event should have a resource");
      t.ok(event.resource.path, "resource should have a path");
      t.ok(event.cursor, "event should have a cursor");
      t.equal(event.eventType, "PUT", "first 10 events should all be PUT");
      t.ok(event.contentHash, "PUT events should have contentHash");
    }
  } finally {
    reader1.releaseLock();
  }

  // === Test 2: Path filtering - /pub/app1/ ===
  t.comment("Test 2: Path filtering - /pub/app1/");

  const stream2 = await sdk
    .eventStream()
    .addUser(userPk, null)
    .path("/pub/app1/")
    .subscribe();

  const events2 = [];
  const reader2 = stream2.getReader();

  try {
    while (true) {
      const { done, value } = await reader2.read();
      if (done) break;
      events2.push(value);
    }

    // Should get 15 PUT + 3 DEL = 18 events from /pub/app1/
    t.equal(
      events2.length,
      18,
      "should receive 18 events from /pub/app1/ (15 PUT + 3 DEL)",
    );

    const putCount = events2.filter((e) => e.eventType === "PUT").length;
    const delCount = events2.filter((e) => e.eventType === "DEL").length;

    t.equal(putCount, 15, "should have 15 PUT events");
    t.equal(delCount, 3, "should have 3 DEL events");

    for (const event of events2) {
      t.ok(
        event.resource.path.includes("/pub/app1/"),
        `event path should contain /pub/app1/: ${event.resource.path}`,
      );
    }
  } finally {
    reader2.releaseLock();
  }

  // === Test 3: Path filtering - /pub/app2/ ===
  t.comment("Test 3: Path filtering - /pub/app2/");

  const stream3 = await sdk
    .eventStream()
    .addUser(userPk, null)
    .path("/pub/app2/")
    .limit(20)
    .subscribe();

  const events3 = [];
  const reader3 = stream3.getReader();

  try {
    while (true) {
      const { done, value } = await reader3.read();
      if (done) break;
      events3.push(value);
    }

    t.equal(events3.length, 10, "should receive 10 events from /pub/app2/");

    for (const event of events3) {
      t.ok(
        event.resource.path.includes("/pub/app2/"),
        `event path should contain /pub/app2/: ${event.resource.path}`,
      );
      t.equal(event.eventType, "PUT", "app2 events should all be PUT");
    }
  } finally {
    reader3.releaseLock();
  }

  // === Test 4: DELETE events structure ===
  t.comment("Test 4: DELETE events structure");

  const stream4 = await sdk
    .eventStream()
    .addUser(userPk, null)
    .path("/pub/app1/")
    .reverse()
    .limit(5)
    .subscribe();

  const events4 = [];
  const reader4 = stream4.getReader();

  try {
    while (true) {
      const { done, value } = await reader4.read();
      if (done) break;
      events4.push(value);
    }

    // In reverse order, the 3 DELETE events should come first
    const delEvents = events4.filter((e) => e.eventType === "DEL");

    t.ok(delEvents.length >= 3, "should have at least 3 DEL events");

    for (const delEvent of delEvents) {
      t.ok(delEvent.resource, "DEL event should have a resource");
      t.ok(delEvent.resource.path, "DEL event resource should have a path");
      t.ok(delEvent.cursor, "DEL event should have a cursor");
      t.notOk(delEvent.contentHash, "DEL event should not have contentHash");
    }
  } finally {
    reader4.releaseLock();
  }

  // === Test 5: Reverse order ===
  t.comment("Test 5: Reverse order");

  const stream5 = await sdk.eventStream().addUser(userPk, null).reverse().limit(10).subscribe();

  const events5 = [];
  const reader5 = stream5.getReader();

  try {
    while (true) {
      const { done, value } = await reader5.read();
      if (done) break;
      events5.push(value);
    }

    t.ok(events5.length > 0, "should receive events in reverse order");

    // In reverse order, cursors should be decreasing
    if (events5.length > 1) {
      const firstCursor = parseInt(events5[0].cursor);
      const lastCursor = parseInt(events5[events5.length - 1].cursor);
      t.ok(
        firstCursor > lastCursor,
        "reverse order: first cursor should be greater than last cursor",
      );
    }

    // First events in reverse should be the DELETEs (most recent)
    t.equal(
      events5[0].eventType,
      "DEL",
      "reverse order: first event should be DEL (most recent)",
    );
  } finally {
    reader5.releaseLock();
  }

  // === Test 6: Cursor-based pagination ===
  t.comment("Test 6: Cursor-based pagination");

  // Get first 5 events
  const streamP1 = await sdk.eventStream().addUser(userPk, null).limit(5).subscribe();

  const firstBatch = [];
  const readerP1 = streamP1.getReader();

  try {
    while (true) {
      const { done, value } = await readerP1.read();
      if (done) break;
      firstBatch.push(value);
    }
  } finally {
    readerP1.releaseLock();
  }

  t.equal(firstBatch.length, 5, "first batch should have 5 events");

  // Get next batch using cursor
  const lastCursor = firstBatch[firstBatch.length - 1].cursor;
  const streamP2 = await sdk
    .eventStream()
    .addUser(userPk, lastCursor)
    .limit(5)
    .subscribe();

  const secondBatch = [];
  const readerP2 = streamP2.getReader();

  try {
    while (true) {
      const { done, value } = await readerP2.read();
      if (done) break;
      secondBatch.push(value);
    }
  } finally {
    readerP2.releaseLock();
  }

  t.ok(secondBatch.length > 0, "second batch should have events");

  // Verify no overlap
  const firstPaths = new Set(firstBatch.map((e) => e.resource.path));
  const secondPaths = secondBatch.map((e) => e.resource.path);

  for (const path of secondPaths) {
    t.notOk(
      firstPaths.has(path),
      `second batch should not contain paths from first batch: ${path}`,
    );
  }

  // === Test 7: ReadableStream iteration ===
  t.comment("Test 7: ReadableStream iteration");

  const stream7 = await sdk
    .eventStream()
    .addUser(userPk, null)
    .path("/pub/photos/")
    .subscribe();

  const events7 = [];
  const reader7 = stream7.getReader();

  try {
    while (true) {
      const { done, value } = await reader7.read();
      if (done) break;
      events7.push(value);
    }

    t.equal(events7.length, 3, "should receive 3 photo events");

    for (const event of events7) {
      t.equal(typeof event.eventType, "string", "eventType should be string");
      t.ok(event.resource, "event should have a resource");
      t.equal(typeof event.resource.path, "string", "path should be string");
      t.equal(typeof event.cursor, "string", "cursor should be string");
      t.ok(event.resource.path.includes("/pub/photos/"), "should be from photos directory");
    }
  } finally {
    reader7.releaseLock();
  }

  // === Test 8: Validation error - live + reverse ===
  t.comment("Test 8: Validation error - live + reverse");

  try {
    await sdk.eventStream().addUser(userPk, null).live().reverse().subscribe();
    t.fail("should throw error when combining live() and reverse()");
  } catch (error) {
    assertPubkyError(t, error);
    t.ok(
      error.message.includes("live mode with reverse"),
      "error message should mention incompatibility",
    );
  }

  t.end();
});

/**
 * Test multi-user event stream subscription.
 */
test("eventStream: multi-user subscription", async (t) => {
  const sdk = Pubky.testnet();

  // Create two users on the same homeserver
  const signer1 = sdk.signer(Keypair.random());
  const signupToken1 = await createSignupToken();
  const session1 = await signer1.signup(HOMESERVER_PUBLICKEY, signupToken1);
  const user1Pk = session1.info.publicKey;

  const signer2 = sdk.signer(Keypair.random());
  const signupToken2 = await createSignupToken();
  const session2 = await signer2.signup(HOMESERVER_PUBLICKEY, signupToken2);
  const user2Pk = session2.info.publicKey;

  // Create events for user1
  for (let i = 0; i < 3; i++) {
    const path = `/pub/user1/file_${i}.txt` as Path;
    await session1.storage.putText(path, `user1 content ${i}`);
  }

  // Create events for user2
  for (let i = 0; i < 2; i++) {
    const path = `/pub/user2/file_${i}.txt` as Path;
    await session2.storage.putText(path, `user2 content ${i}`);
  }

  await sleep(500); // Wait for events to be recorded

  // === Test: Subscribe to both users ===
  t.comment("Test: Multi-user subscription");

  const stream = await sdk
    .eventStream()
    .addUser(user1Pk, null)
    .addUser(user2Pk, null)
    .subscribe();

  const events = [];
  const reader = stream.getReader();

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      events.push(value);
    }

    // Should receive events from both users (3 + 2 = 5)
    t.equal(events.length, 5, "should receive 5 events total from both users");

    // Verify we have events from both users
    const user1Events = events.filter(
      (e) => e.resource.owner.z32() === user1Pk.z32(),
    );
    const user2Events = events.filter(
      (e) => e.resource.owner.z32() === user2Pk.z32(),
    );

    t.equal(user1Events.length, 3, "should have 3 events from user1");
    t.equal(user2Events.length, 2, "should have 2 events from user2");

    // Verify paths are correct for each user
    for (const event of user1Events) {
      t.ok(
        event.resource.path.includes("/pub/user1/"),
        `user1 event should have correct path: ${event.resource.path}`,
      );
    }
    for (const event of user2Events) {
      t.ok(
        event.resource.path.includes("/pub/user2/"),
        `user2 event should have correct path: ${event.resource.path}`,
      );
    }
  } finally {
    reader.releaseLock();
  }

  // === Test: Update cursor for existing user ===
  t.comment("Test: Updating cursor for existing user");

  // Get first batch to establish a cursor
  const streamBatch1 = await sdk
    .eventStream()
    .addUser(user1Pk, null)
    .limit(2)
    .subscribe();

  const batch1 = [];
  const reader1 = streamBatch1.getReader();
  try {
    while (true) {
      const { done, value } = await reader1.read();
      if (done) break;
      batch1.push(value);
    }
  } finally {
    reader1.releaseLock();
  }

  const cursor = batch1[batch1.length - 1].cursor;

  // Add same user with updated cursor - should work without error
  const streamBatch2 = await sdk
    .eventStream()
    .addUser(user1Pk, null)
    .addUser(user1Pk, cursor) // Adding same user again updates the cursor
    .limit(5)
    .subscribe();

  const batch2 = [];
  const reader2 = streamBatch2.getReader();
  try {
    while (true) {
      const { done, value } = await reader2.read();
      if (done) break;
      batch2.push(value);
    }
  } finally {
    reader2.releaseLock();
  }

  // Should get remaining events after cursor
  t.ok(batch2.length > 0, "should receive events after cursor update");
  t.ok(
    batch2.length < batch1.length + 3,
    "second batch should have fewer events than if starting from beginning",
  );

  t.end();
});

/**
 * Test error handling for non-existent user.
 */
test("eventStream: invalid user key", async (t) => {
  const sdk = Pubky.testnet();

  const fakeUser = Keypair.random().publicKey;

  try {
    // Try to subscribe to events for a user that doesn't exist
    await sdk.eventStream().addUser(fakeUser, null).limit(10).subscribe();

    // If the homeserver can't be resolved, it should fail
    // But if it goes through, we should be able to read from stream
    t.pass("subscribe succeeded (homeserver resolved)");
  } catch (error) {
    // Expected to fail if user doesn't have a homeserver
    assertPubkyError(t, error);
    t.ok(
      error.message.includes("homeserver") || error.message.includes("resolve"),
      "error should mention homeserver resolution failure",
    );
  }

  t.end();
});
