import test from 'tape'

import { PubkyClient } from '../index.cjs'




test('new PubkyClient() without config', async (t) => {
  const client = new PubkyClient(); // Should always work
  t.ok(client, "should create a client");
});

test('new PubkyClient() with config', async (t) => {
  const client = new PubkyClient({
    pkarr: {
      relays: ['http://localhost:15412/relay'],
      requestTimeout: 1000
    },
    userMaxRecordAge: 1000
  });
  t.ok(client, "should create a client");
});

test('new PubkyClient() partial config', async (t) => {
  const client = new PubkyClient({
    userMaxRecordAge: 1000
  });
  t.ok(client, "should create a client");
});

test('new PubkyClient() with faulty config', async (t) => {
  t.throws(() => new PubkyClient({
    userMaxRecordAge: 0 // Zero is invalid
  }), "should throw an error");
});