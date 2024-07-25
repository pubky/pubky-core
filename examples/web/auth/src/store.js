import { createMutable } from 'solid-js/store'
// import z32 from 'z32'
// import { Result } from '@pubky/common'
// import { Level } from 'level'
// import Client from '@pubky/client'
//
// import { recoveryFile } from './sdk/recovery.js'
//
// // In real application it should be the server's Pkarr Id
// const DEFAULT_HOME_SERVER = 'http://localhost:7259'
// const DEFAULT_RELAY = 'https://relay.pkarr.org'

class Store {
  constructor() {
    // this.db = new Level('app-db', { keyEncoding: 'utf8', valueEncoding: 'json' })
    // this.currentUser = null
    //
    // this.pubkyClient = new Client(DEFAULT_HOME_SERVER, { relay: DEFAULT_RELAY })
    //
    // this.DEFAULT_HOME_SERVER = DEFAULT_HOME_SERVER
  }

  // getUsers() {
  //   try {
  //     return JSON.parse(global.localStorage.getItem('users')) || []
  //   } catch {
  //     return []
  //   }
  // }

  getCurrentUser() {
    try {
      return JSON.parse(global.localStorage.getItem('currentUser'))
    } catch {
      return null
    }
  }

  // setCurrentUser(user) {
  //   const users = this.getUsers()
  //
  //   if (!users?.map(user => user.id).includes(user.id)) {
  //     global.localStorage.setItem('users', JSON.stringify([
  //       ...users,
  //       user
  //     ]))
  //   }
  //
  //   global.localStorage.setItem('currentUser', JSON.stringify(user))
  // }
  //
  // removeCurrentUser() {
  //   global.localStorage.removeItem('currentUser')
  // }
  //
  // /**
  //  * @param {string} name
  //  * @param {string} passphrase
  //  *
  //  * @returns {Promise<Result<{
  //  *  recoveryFile: Uint8Array,
  //  *  filename: string,
  //  *  signupUrl: string
  //  * }>>}
  //  */
  // async createAccount(name, passphrase) {
  //   await this.pubkyClient.ready()
  //
  //   const seed = Client.crypto.generateSeed()
  //
  //   const keypair = Client.crypto.generateKeyPair(seed)
  //   Client.crypto.zeroize(keypair.secretKey)
  //
  //   const userId = z32.encode(keypair.publicKey)
  //
  //   const recoveryFileAndFilename = await recoveryFile(name, seed, passphrase)
  //
  //   const signedUp = await this.pubkyClient.signup(seed)
  //   if (signedUp.isErr()) return signedUp
  //
  //   Client.crypto.zeroize(seed)
  //
  //   return Result.Ok({
  //     userId,
  //     ...recoveryFileAndFilename
  //   })
  // }
}

export default createMutable(new Store())
