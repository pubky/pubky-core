import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.js'

test('seed auth', async (t) => {
  const client = new PubkyClient()

  const keypair = Keypair.random()
  const publicKey = keypair.public_key()

  const homeserver = PublicKey.try_from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
  await client.signup(keypair, homeserver)

  const session = await client.session(publicKey)
  t.ok(session)

  {
    await client.signout(publicKey)

    const session = await client.session(publicKey)
    t.notOk(session)
  }

  {
    await client.signin(keypair)

    const session = await client.session(publicKey)
    t.ok(session)
  }
})