import test from 'brittle'

import { PubkyClient, Keypair, PublicKey } from '../index.js'

test('seed auth', async (t) => {

  let client = new PubkyClient();

  let keypair = Keypair.random();
  let homeserver = PublicKey.try_from("8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo");

  await client.signup(keypair, homeserver);

  // const client = new Client(
  //   homeserver.homeserver.pkarr.serverPkarr.publicKey(),
  //   {
  //     relay: homeserver.testnet.relay
  //   }
  // )
  // await client.ready()
  //
  // const seed = Client.crypto.generateSeed()
  // const keypair = Client.crypto.generateKeyPair(seed)
  // const expectedUserId = keypair.public_key().to_string()
  //
  // const userIdResult = await client.signup(seed)
  // t.ok(userIdResult.isOk(), userIdResult.error)
  //
  // const userId = userIdResult.value
  // t.is(userId, expectedUserId)
  //
  // const session = await client.session()
  // t.ok(session?.users[userId])
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
