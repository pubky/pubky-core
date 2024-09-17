import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.cjs'

const TLD = '8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo';

// TODO: test HTTPs too somehow.

test("basic fetch", async (t) => {
  let client = PubkyClient.testnet();

  let response = await client.fetch(`https://${TLD}/`, new Uint8Array([]));

  t.equal(response.status, 200);
})

