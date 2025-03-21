import test from 'tape'

import { Keypair } from '../index.cjs'

test('generate keys from a seed', async (t) => {
  const secretkey = Buffer.from('5aa93b299a343aa2691739771f2b5b85e740ca14c685793d67870f88fa89dc51', 'hex')

  const keypair = Keypair.fromSecretKey(secretkey)

  const publicKey = keypair.publicKey()

  t.is(publicKey.z32(), 'gcumbhd7sqit6nn457jxmrwqx9pyymqwamnarekgo3xppqo6a19o')
})

test('fromSecretKey error', async (t) => {
  const secretkey = Buffer.from('5aa93b299a343aa2691739771f2b5b', 'hex')


  t.throws(() => Keypair.fromSecretKey(null), /Expected secret_key to be an instance of Uint8Array/)
  t.throws(() => Keypair.fromSecretKey(secretkey), /Expected secret_key to be 32 bytes, got 15/)
})
