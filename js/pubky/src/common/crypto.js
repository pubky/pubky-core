//! Crypeo functions

import sodium from 'sodium-universal'
import z32 from 'z32'

// Blake3

/** @type {import('blake3-wasm')} */
let loadedBlake3

const loadBlake3 = async () => {
  if (loadedBlake3) return loadedBlake3
  // @ts-ignore
  loadedBlake3 = await import('blake3-wasm').then(b3 => b3.load().then(() => b3))

  return loadedBlake3
}

loadBlake3()

/**
 * It will return null if blake3 is not loaded yet!
 *
 * @param {Buffer} message
 *
 * @returns {Buffer | null}
 */
export const hash = (message) => {
  return loadedBlake3?.createHash().update(message).digest()
}

// Random
export const randomBytes = (n = 32) => {
  const buf = Buffer.alloc(n)
  sodium.randombytes_buf(buf)
  return buf
}

/// Keypairs

/**
 * @param {Buffer} buf
 */
export const zeroize = (buf) => {
  buf.fill(0)
}

export class KeyPair {
  #publicKey
  #secretKey

  /**
   * @param {Buffer} seed
   */
  constructor (seed) {
    this.#publicKey = Buffer.allocUnsafe(sodium.crypto_sign_PUBLICKEYBYTES)
    this.#secretKey = Buffer.allocUnsafe(sodium.crypto_sign_SECRETKEYBYTES)

    if (seed) sodium.crypto_sign_seed_keypair(this.#publicKey, this.#secretKey, seed)
    else sodium.crypto_sign_keypair(this.#publicKey, this.#secretKey)
  }

  static random () {
    const seed = randomBytes(32)

    return new KeyPair(seed)
  }

  zeroize () {
    zeroize(this.#secretKey)
    this.secretKey = null
  }

  publicKey () {
    return new PublicKey(this.#publicKey)
  }

  secretKey () {
    return this.#secretKey
  }

  /**
   * @param {Uint8Array} message
   */
  sign (message) {
    const signature = Buffer.alloc(sodium.crypto_sign_BYTES)
    sodium.crypto_sign_detached(signature, message, this.#secretKey)

    return signature
  }
}

export class PublicKey {
  /**
   * @param {Buffer} bytes
   */
  constructor (bytes) {
    this.bytes = bytes
  }

  /**
   * @param {string} string
   * @returns {Error | PublicKey}
   */
  static fromString (string) {
    if (string.length !== 52) {
      return new Error('Invalid PublicKey string, expected 52 characters, got: ' + string.length)
    }

    try {
      return new PublicKey(z32.decode(string))
    } catch (error) {
      return error
    }
  }

  /**
   * @param {Buffer} signature
   * @param {Buffer} message
   */
  verify (signature, message) {
    const valid = sodium.crypto_sign_verify_detached(signature, message, this.bytes)
    if (!valid) return new Error('Invalid signature')

    return true
  }

  toString () {
    return z32.encode(this.bytes)
  }
}
