import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.js'

test('seed auth', async (t) => {

  let client = new PubkyClient();

  let keypair = Keypair.random();

  let homeserver = PublicKey.try_from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo");
  await client.signup(keypair, homeserver);

  t.ok(true);

  // const session = await client.session()
  // t.ok(session)
  //
  // {
  //   await client.logout(userId)
  //
  //   const session = await client.session()
  //   t.absent(session?.users?.[userId])
  // }
  //
  // {
  //   await client.login(seed)
  //
  //   const session = await client.session()
  //   t.ok(session?.users[userId])
  // }
})
