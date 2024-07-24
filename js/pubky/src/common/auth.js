import { Timestamp } from './timestamp.js'
import * as namespaces from './namespaces.js'
import * as crypto from './crypto.js'

// 30 seconds
const TIME_INTERVAL = 30 * 1000000

export class AuthnSignature {
  /**
   * @param {number} time
   * @param {import ('./crypto.js').KeyPair} signer
   * @param {import ('./crypto.js').PublicKey} audience
   * @param {Buffer} [token]
   */
  constructor (time, signer, audience, token = crypto.randomBytes()) {
    const timeStep = Math.floor(time / TIME_INTERVAL)

    const tokenHash = crypto.hash(token)

    const timeStepBytes = Buffer.allocUnsafe(8)
    timeStepBytes.writeBigUint64BE(BigInt(timeStep))

    const signature = signer.sign(signable(
      signer.publicKey().bytes,
      audience.bytes,
      timeStepBytes,
      tokenHash
    ))

    this.bytes = Buffer.concat([
      signature,
      tokenHash
    ])
  }

  /**
   * @param {import ('./crypto.js').KeyPair} signer
   * @param {import ('./crypto.js').PublicKey} audience
   * @param {Buffer} [token]
   */
  static sign (signer, audience, token = crypto.randomBytes()) {
    const time = Timestamp.now().microseconds

    return new AuthnSignature(time, signer, audience, token)
  }

  asBytes () {
    return new Uint8Array(this.bytes)
  }
}

export class AuthnVerifier {
  #audience
  /** @type {Array<Buffer>} */
  #seen

  /**
   * @param {crypto.PublicKey} audience
   */
  constructor (audience) {
    this.#audience = audience

    this.#seen = []
  }

  #gc () {
    const threshold = Timestamp.now().microseconds
    const threshouldStep = Math.floor(threshold / TIME_INTERVAL) - 2

    const thresholdBytes = Buffer.allocUnsafe(8)
    thresholdBytes.writeBigUint64BE(BigInt(threshouldStep))

    let count = 0

    for (let i = 0; i < this.#seen.length; i++) {
      if (this.#seen[i].subarray(0, 8).compare(thresholdBytes) > 0) {
        break
      }
      count = i
    }

    this.#seen.splice(0, count)
  }

  /**
   * @param {Buffer} bytes
   * @param {crypto.PublicKey} signer
   *
   * @returns {true | Error}
   */
  verify (bytes, signer) {
    this.#gc()

    if (bytes.length !== 96) {
      throw new Error(`InvalidLength: ${bytes.length}`)
    }

    const signature = bytes.subarray(0, 64)
    const tokenHash = bytes.subarray(64)

    const now = Timestamp.now().microseconds
    const past = now - TIME_INTERVAL
    const future = now + TIME_INTERVAL

    let result = verifyAt.call(this, now)

    if (!(result instanceof Error)) {
      return result
    } else if (result.toString() === 'Error: AuthnSignature already used') {
      return result
    }

    result = verifyAt.call(this, past)

    if (!(result instanceof Error)) {
      return result
    } else if (result.toString() === 'Error: AuthnSignature already used') {
      return result
    }

    return verifyAt.call(this, future)

    /**
     * @param {number} time
     */
    function verifyAt (time) {
      const timeStep = Math.floor(time / TIME_INTERVAL)

      const timeStepBytes = Buffer.allocUnsafe(8)
      timeStepBytes.writeBigUint64BE(BigInt(timeStep))

      const result = signer.verify(signature, signable(signer.bytes, this.#audience.bytes, timeStepBytes, tokenHash))

      const candidate = Buffer.concat([
        timeStepBytes,
        tokenHash
      ])

      if (!(result instanceof Error)) {
        const index = binarySearch(this.#seen, timeStepBytes)

        if (this.#seen[index]?.equals(candidate)) {
          return new Error('AuthnSignature already used')
        }

        this.#seen.splice(~index, 0, candidate)

        return
      }

      return result
    }
  }
}

/**
 * @param {Array<Buffer>} arr
 */
function binarySearch (arr, element) {
  let left = 0
  let right = arr.length - 1

  while (left <= right) {
    const mid = Math.floor((left + right) / 2)

    const comparison = arr[mid].subarray(0, 8).compare(element.subarray(0, 8))

    if (comparison === 0) {
      return mid
    } else if (comparison < 0) {
      left = mid + 1
    } else {
      right = mid - 1
    }
  }

  return left // Element not found, return the index where it should be inserted
}

/**
 * @param {Buffer} signer
 * @param {Buffer} audience
 * @param {Buffer} timeStepBytes
 * @param {Buffer} tokenHash
 */
function signable (signer, audience, timeStepBytes, tokenHash) {
  return Buffer.concat([
    namespaces.PUBKY_AUTHN,
    timeStepBytes,
    signer,
    audience,
    tokenHash
  ])
}
