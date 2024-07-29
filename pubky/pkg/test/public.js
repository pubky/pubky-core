import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.js'

test('public: put/get', async (t) => {
  const client = new PubkyClient().setPkarrRelays(["http://localhost:15411/pkarr"])

  const keypair = Keypair.random();

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo');
  await client.signup(keypair, homeserver);

  const publicKey = keypair.public_key();

  const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

  // PUT public data, by authorized client
  await client.put(publicKey, "/pub/example.com/arbitrary", body);


  // GET public data without signup or signin
  {
    const client = new PubkyClient().setPkarrRelays(["http://localhost:15411/pkarr"])

    let response = await client.get(publicKey, "/pub/example.com/arbitrary");

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
