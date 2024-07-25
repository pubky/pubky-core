import { PubkyClient, Keypair } from '@synonymdev/pubky'

let keypair = Keypair.from_secret_key(new Uint8Array(32).fill(0))
console.log(keypair)

const client = new PubkyClient()

console.log(client)

const x = client.signup(keypair, "foo.com")
