import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.js'

test('public: put/get', async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random();

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo');
  await client.signup(keypair, homeserver);

  const publicKey = keypair.public_key();

  let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

  const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

  // PUT public data, by authorized client
  await client.put(url, body);


  // GET public data without signup or signin
  {
    const client = PubkyClient.testnet();

    let response = await client.get(url);

    t.ok(Buffer.from(response).equals(body))
  }

  // // DELETE public data, by authorized client
  // await client.delete(publicKey, "/pub/example.com/arbitrary");
  //
  //
  // // GET public data without signup or signin
  // {
  //   const client = new PubkyClient();
  //
  //   let response = await client.get(publicKey, "/pub/example.com/arbitrary");
  //
  //   t.notOk(response)
  // }
})

test.skip("not found")

test.skip("unauthorized")
