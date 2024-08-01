import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.cjs'

test('auth', async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
  await client.signup(keypair, homeserver)

  const session = await client.session(publicKey)
  t.ok(session, "signup")

  {
    await client.signout(publicKey)

    const session = await client.session(publicKey)
    t.notOk(session, "singout")
  }

  {
    await client.signin(keypair)

    const session = await client.session(publicKey)
    t.ok(session, "signin")
  }
})
