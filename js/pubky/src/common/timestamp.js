import { CrockfordBase32 } from 'crockford-base32'
import { randomBytes } from './crypto.js'

const clockId = randomBytes(1).readUintBE(0, 1)
let latest = 0

export class Timestamp {
  /**
   * @param {number} microseconds - u64 microseconds
   */
  constructor (microseconds) {
    /** microseconds as u64 */
    this.microseconds = microseconds
  }

  static now () {
    const now = Date.now()
    latest = Math.max(now, latest + 1)

    return new Timestamp((latest * 1000) + clockId)
  }

  /**
   * @param {string} string
   */
  static fromString (string) {
    const microseconds = Number(CrockfordBase32.decode(string, { asNumber: true }))
    return new Timestamp(microseconds)
  }

  /**
   * @param {Date} date
   */
  static fromDate (date) {
    const microseconds = Number(date) * 1000
    return new Timestamp(microseconds)
  }

  toString () {
    return CrockfordBase32.encode(this.microseconds)
  }

  toDate () {
    return new Date(this.microseconds / 1000)
  }

  intoBytes () {
    const buffer = Buffer.allocUnsafe(8)
    buffer.writeBigUint64BE(BigInt(this.microseconds), 0)

    return buffer
  }
}
