import test from 'tape'

import { Client } from '../index.cjs'




test('new Client() without config', async (t) => {
  const client = new Client(); // Should always work
  t.ok(client, "should create a client");
});

test('new Client() with config', async (t) => {
  const client = new Client({
    pkarr: {
      relays: ['http://localhost:15412/relay'],
      requestTimeout: 1000
    },
    userMaxRecordAge: 1000
  });
  t.ok(client, "should create a client");
});

test('new Client() partial config', async (t) => {
  const client = new Client({
    userMaxRecordAge: 1000
  });
  t.ok(client, "should create a client");
});

test('new Client() with faulty config', async (t) => {
  t.throws(() => new Client({
    userMaxRecordAge: 0 // Zero is invalid
  }), "should throw an error");
});