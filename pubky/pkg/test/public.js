import test from 'tape'

import { PubkyClient, Keypair, PublicKey } from '../index.cjs'

test('public: put/get', async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random();

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo');
  await client.signup(keypair, homeserver);

  const publicKey = keypair.publicKey();

  let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

  const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

  // PUT public data, by authorized client
  await client.put(url, body);

  const otherClient = PubkyClient.testnet();

  // GET public data without signup or signin
  {
    let response = await otherClient.get(url);

    t.ok(Buffer.from(response).equals(body))
  }

  // DELETE public data, by authorized client
  await client.delete(url);


  // GET public data without signup or signin
  {
    let response = await otherClient.get(url);

    t.notOk(response)
  }
})

test("not found", async (t) => {
  const client = PubkyClient.testnet();


  const keypair = Keypair.random();

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo');
  await client.signup(keypair, homeserver);

  const publicKey = keypair.publicKey();

  let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

  let result = await client.get(url).catch(e => e);

  t.notOk(result);
})

test("unauthorized", async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
  await client.signup(keypair, homeserver)

  const session = await client.session(publicKey)
  t.ok(session, "signup")

  await client.signout(publicKey)

  const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

  let url = `pubky://${publicKey.z32()}/pub/example.com/arbitrary`;

  // PUT public data, by authorized client
  let result = await client.put(url, body).catch(e => e);

  t.ok(result instanceof Error);
  t.is(
    result.message,
    `HTTP status client error (401 Unauthorized) for url (http://localhost:15411/${publicKey.z32()}/pub/example.com/arbitrary)`
  )
})

test("forbidden", async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
  await client.signup(keypair, homeserver)

  const session = await client.session(publicKey)
  t.ok(session, "signup")

  const body = Buffer.from(JSON.stringify({ foo: 'bar' }))

  let url = `pubky://${publicKey.z32()}/priv/example.com/arbitrary`;

  // PUT public data, by authorized client
  let result = await client.put(url, body).catch(e => e);

  t.ok(result instanceof Error);
  t.is(
    result.message,
    `HTTP status client error (403 Forbidden) for url (http://localhost:15411/${publicKey.z32()}/priv/example.com/arbitrary)`
  )
})

test("list", async (t) => {
  const client = PubkyClient.testnet();

  const keypair = Keypair.random()
  const publicKey = keypair.publicKey()
  const pubky = publicKey.z32()

  const homeserver = PublicKey.from('8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo')
  await client.signup(keypair, homeserver)



  let urls = [
    `pubky://${pubky}/pub/a.wrong/a.txt`,
    `pubky://${pubky}/pub/example.com/a.txt`,
    `pubky://${pubky}/pub/example.com/b.txt`,
    `pubky://${pubky}/pub/example.wrong/a.txt`,
    `pubky://${pubky}/pub/example.com/c.txt`,
    `pubky://${pubky}/pub/example.com/d.txt`,
    `pubky://${pubky}/pub/z.wrong/a.txt`,
  ]

  for (let url of urls) {
    await client.put(url, Buffer.from(""));
  }

  let url = `pubky://${pubky}/pub/example.com/`;

  {
    let list = await client.list(url);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/a.txt`,
        `pubky://${pubky}/pub/example.com/b.txt`,
        `pubky://${pubky}/pub/example.com/c.txt`,
        `pubky://${pubky}/pub/example.com/d.txt`,

      ],
      "normal list with no limit or cursor"
    );
  }

  {
    let list = await client.list(url, null, null, 2);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/a.txt`,
        `pubky://${pubky}/pub/example.com/b.txt`,

      ],
      "normal list with limit but no cursor"
    );
  }

  {
    let list = await client.list(url, "a.txt", null, 2);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/b.txt`,
        `pubky://${pubky}/pub/example.com/c.txt`,

      ],
      "normal list with limit and a suffix cursor"
    );
  }

  {
    let list = await client.list(url, `pubky://${pubky}/pub/example.com/a.txt`, null, 2);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/b.txt`,
        `pubky://${pubky}/pub/example.com/c.txt`,

      ],
      "normal list with limit and a full url cursor"
    );
  }


  {
    let list = await client.list(url, null, true);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/d.txt`,
        `pubky://${pubky}/pub/example.com/c.txt`,
        `pubky://${pubky}/pub/example.com/b.txt`,
        `pubky://${pubky}/pub/example.com/a.txt`,

      ],
      "reverse list with no limit or cursor"
    );
  }

  {
    let list = await client.list(url, null, true, 2);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/d.txt`,
        `pubky://${pubky}/pub/example.com/c.txt`,

      ],
      "reverse list with limit but no cursor"
    );
  }

  {
    let list = await client.list(url, "d.txt", true, 2);

    t.deepEqual(
      list,
      [
        `pubky://${pubky}/pub/example.com/c.txt`,
        `pubky://${pubky}/pub/example.com/b.txt`,

      ],
      "reverse list with limit and a suffix cursor"
    );
  }
})