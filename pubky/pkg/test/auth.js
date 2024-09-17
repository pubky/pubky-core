import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.cjs'

const Homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')

test('auth', async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()

  await client.signup(keypair, Homeserver)

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

test("3rd party signin", async (t) => {
  let keypair = Keypair.random();
  let pubky = keypair.publicKey().z32();

  // Third party app side
  let capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r";
  let client = PubkyClient.testnet();
  let [pubkyauth_url, pubkyauthResponse] = client
    .authRequest("https://demo.httprelay.io/link", capabilities);

  if (globalThis.document) {
    // Skip `sendAuthToken` in browser
    // TODO: figure out why does it fail in browser unit tests
    // but not in real browser (check pubky-auth-widget.js commented part)
    return
  }

  // Authenticator side
  {
    let client = PubkyClient.testnet();

    await client.signup(keypair, Homeserver);

    await client.sendAuthToken(keypair, pubkyauth_url)
  }

  let authedPubky = await pubkyauthResponse;

  t.is(authedPubky.z32(), pubky);

  let session = await client.session(authedPubky);
  t.deepEqual(session.capabilities(), capabilities.split(','))
})
