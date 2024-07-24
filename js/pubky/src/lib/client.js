import Pkarr, { SignedPacket } from 'pkarr'
import { URL } from 'url'

// import { AuthnSignature, crypto } from '@pubky/common'

import * as crypto from '../common/crypto.js'

// import { Pubky } from './pubky.js'
import fetch from './fetch.js'


const DEFAULT_PKARR_RELAY = new URL('https://relay.pkarr.org')

export class PubkyClient {
  // TODO: use DHT in nodejs
  #pkarrRelay

  crypto = crypto
  static crypto = crypto

  /**
   * @param {object} [options={}]
   * @param {URL} [options.pkarrRelay]
   *
   * @param {{relay: string, bootstrap: Array<{host: string, port: number}>}} [options.testnet]
   */
  constructor(options = {}) {
    this.#pkarrRelay = options.pkarrRelay || DEFAULT_PKARR_RELAY
  }

  /**
   * Publish the SVCB record for `_pubky.<public_key>`.
   * @param {crypto.KeyPair} keypair 
   * @param {String} host 
   */
  async publishPubkyHomeserver(keypair, host) {

    let existing = await (async () => {
      try {
        return (await Pkarr.relayGet(this.#pkarrRelay.toString(), keypair.publicKey().bytes)).packet()
      } catch (error) {
        return {
          id: 0,
          type: 'response',
          flags: 0,
          answers: []
        }
      }
    })();

    let answers = [
    ];

    for (let answer of existing.answers) {
      if (!answer.name.startsWith("_pubky")) {
        answers.push(answer)
      }
    }

    let signedPacket = SignedPacket.fromPacket(keypair, {
      id: 0,
      type: 'response',
      flags: 0,
      answers: [
        ...answers,
        {
          name: '_pubky.', type: 'SVCB', ttl: 7200, data:

            Buffer.from(

            )

        }
      ]
    })

    // let mut packet = Packet:: new_reply(0);
    //
    //     if let Some(existing) = self.pkarr.resolve(& keypair.public_key()) ? {
    //       for answer in existing.packet().answers.iter().cloned() {
    //       if !answer.name.to_string().starts_with("_pubky") {
    //         packet.answers.push(answer.into_owned())
    //       }
    //     }
    //   }
    //
    // let svcb = SVCB::new (0, host.try_into() ?);
    //
    // packet.answers.push(pkarr:: dns:: ResourceRecord:: new (
    //   "_pubky".try_into().unwrap(),
    //   pkarr:: dns:: CLASS:: IN,
    //   60 * 60,
    //   pkarr:: dns:: rdata:: RData:: SVCB(svcb),
    // ));
    //
    // let signed_packet = SignedPacket:: from_packet(keypair, & packet) ?;
    //
    // self.pkarr.publish(& signed_packet) ?;
  }
}

export default PubkyClient
