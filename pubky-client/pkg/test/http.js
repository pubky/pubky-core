import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.cjs'

const TLD = '8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo';

test("basic fetch", async (t) => {
  let client = PubkyClient.testnet();

  // Normal TLD
  {
    let response = await client.fetch(`https://google.com/`);

    t.equal(response.status, 200);
  }


  // Pubky
  let response = await client.fetch(`https://${TLD}/`);

  t.equal(response.status, 200);
})

test("fetch failed", async (t) => {

  let client = PubkyClient.testnet();

  // Normal TLD
  {
    let response = await client.fetch(`https://nonexistent.domain/`).catch(e => e);

    t.ok(response instanceof Error);
  }


  // Pubky
  let response = await client.fetch(`https://1pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ew1/`).catch(e => e);

  t.ok(response instanceof Error);
})

