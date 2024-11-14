import test from 'tape'

import { Client, Keypair, PublicKey } from '../index.cjs'

const TLD = '8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo';

// TODO: test HTTPs too somehow.

test.skip("basic fetch", async (t) => {
  let client = Client.testnet();

  // Normal TLD
  {

    let response = await client.fetch(`http://relay.pkarr.org/`);

    t.equal(response.status, 200);
  }


  // Pubky
  let response = await client.fetch(`http://${TLD}/`);

  t.equal(response.status, 200);
})

