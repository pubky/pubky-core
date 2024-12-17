import test from 'tape'

import { Client, Keypair, PublicKey } from '../index.cjs'

const HOMESERVER_PUBLICKEY = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
const TESTNET_HTTP_RELAY = "http://localhost:15412/link";

// TODO: test multiple users in wasm
  
test('auth', async (t) => {
  const client = Client.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()

  await client.signup(keypair, HOMESERVER_PUBLICKEY )

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
  let client = Client.testnet();
  let [pubkyauth_url, pubkyauthResponse] = client
    .authRequest(TESTNET_HTTP_RELAY, capabilities);

  if (globalThis.document) {
    // Skip `sendAuthToken` in browser
    // TODO: figure out why does it fail in browser unit tests
    // but not in real browser (check pubky-auth-widget.js commented part)
    return
  }

  // Authenticator side
  {
    let client = Client.testnet();

    await client.signup(keypair, HOMESERVER_PUBLICKEY);

    await client.sendAuthToken(keypair, pubkyauth_url)
  }

  let authedPubky = await pubkyauthResponse;

  t.is(authedPubky.z32(), pubky);

  let session = await client.session(authedPubky);
  t.deepEqual(session.capabilities(), capabilities.split(','))
})
