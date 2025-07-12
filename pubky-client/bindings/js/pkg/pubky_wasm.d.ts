/* tslint:disable */
/* eslint-disable */
export function setLogLevel(level: string): void;
/**
 * Create a recovery file of the `keypair`, containing the secret key encrypted
 * using the `passphrase`.
 */
export function createRecoveryFile(keypair: Keypair, passphrase: string): Uint8Array;
/**
 * Create a recovery file of the `keypair`, containing the secret key encrypted
 * using the `passphrase`.
 */
export function decryptRecoveryFile(recovery_file: Uint8Array, passphrase: string): Keypair;
/**
 * Pkarr Config
 */
export interface PkarrConfig {
    /**
     * The list of relays to access the DHT with.
     */
    relays: string[] | null;
    /**
     * The timeout for DHT requests in milliseconds.
     * Default is 2000ms.
     */
    requestTimeout: NonZeroU64 | null;
}

/**
 * Pubky Client Config
 */
export interface PubkyClientConfig {
    /**
     * Configuration on how to access pkarr packets on the mainline DHT.
     */
    pkarr: PkarrConfig | null;
    /**
     * The maximum age of a record in seconds.
     * If the user pkarr record is older than this, it will be automatically refreshed.
     */
    userMaxRecordAge: NonZeroU64 | null;
}

export class AuthRequest {
  private constructor();
  free(): void;
  /**
   * Returns the Pubky Auth url, which you should show to the user
   * to request an authentication or authorization token.
   *
   * Wait for this token using `this.response()`.
   */
  url(): string;
  /**
   * Wait for the user to send an authentication or authorization proof.
   *
   * If successful, you should expect an instance of [PublicKey]
   *
   * Otherwise it will throw an error.
   */
  response(): Promise<PublicKey>;
}
export class Client {
  free(): void;
  /**
   * Create a new Pubky Client with an optional configuration.
   */
  constructor(config_opt?: PubkyClientConfig | null);
  /**
   * Create a client with with configurations appropriate for local testing:
   * - set Pkarr relays to `["http://localhost:15411"]` instead of default relay.
   * - transform `pubky://<pkarr public key>` to `http://<pkarr public key` instead of `https:`
   *     and read the homeserver HTTP port from the [reserved service parameter key](pubky_common::constants::reserved_param_keys::HTTP_PORT)
   */
  static testnet(): Client;
  fetch(url: string, init?: any | null): Promise<Promise<any>>;
  /**
   * Returns a list of Pubky urls (as strings).
   *
   * - `url`:     The Pubky url (string) to the directory you want to list its content.
   * - `cursor`:  Either a full `pubky://` Url (from previous list response),
   *                 or a path (to a file or directory) relative to the `url`
   * - `reverse`: List in reverse order
   * - `limit`    Limit the number of urls in the response
   * - `shallow`: List directories and files, instead of flat list of files.
   */
  list(url: string, cursor?: string | null, reverse?: boolean | null, limit?: number | null, shallow?: boolean | null): Promise<Array<any>>;
  /**
   * Signup to a homeserver and update Pkarr accordingly.
   *
   * The homeserver is a Pkarr domain name, where the TLD is a Pkarr public key
   * for example "pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
   */
  signup(keypair: Keypair, homeserver: PublicKey, signup_token?: string | null): Promise<Session>;
  /**
   * Check the current session for a given Pubky in its homeserver.
   *
   * Returns [Session] or `None` (if received `404 NOT_FOUND`),
   * or throws the received error if the response has any other `>=400` status code.
   */
  session(pubky: PublicKey): Promise<Session | undefined>;
  /**
   * Signout from a homeserver.
   */
  signout(pubky: PublicKey): Promise<void>;
  /**
   * Signin to a homeserver using the root Keypair.
   */
  signin(keypair: Keypair): Promise<void>;
  /**
   * Return `pubkyauth://` url and wait for the incoming [AuthToken]
   * verifying that AuthToken, and if capabilities were requested, signing in to
   * the Pubky's homeserver and returning the [Session] information.
   *
   * Returns a [AuthRequest]
   */
  authRequest(relay: string, capabilities: string): AuthRequest;
  /**
   * Sign an [pubky_common::auth::AuthToken], encrypt it and send it to the
   * source of the pubkyauth request url.
   */
  sendAuthToken(keypair: Keypair, pubkyauth_url: string): Promise<void>;
  /**
   * Get the homeserver id for a given Pubky public key.
   * Looks up the pkarr packet for the given public key and returns the content of the first `_pubky` SVCB record.
   * Throws an error if no homeserver is found.
   */
  getHomeserver(public_key: PublicKey): Promise<PublicKey>;
  /**
   * Republish the user's PKarr record pointing to their homeserver.
   *
   * This method will republish the record if no record exists or if the existing record
   * is older than 6 hours.
   *
   * The method is intended for clients and key managers (e.g., pubky-ring) to
   * keep the records of active users fresh and available in the DHT and relays.
   * It is intended to be used only after failed signin due to homeserver resolution
   * failure. This method is lighter than performing a re-signup into the last known
   * homeserver, but does not return a session token, so a signin must be done after
   * republishing. On a failed signin due to homeserver resolution failure, a key
   * manager should always attempt to republish the last known homeserver.
   */
  republishHomeserver(keypair: Keypair, host: PublicKey): Promise<void>;
}
export class Keypair {
  private constructor();
  free(): void;
  /**
   * Generate a random [Keypair]
   */
  static random(): Keypair;
  /**
   * Generate a [Keypair] from a secret key.
   */
  static fromSecretKey(secret_key: Uint8Array): Keypair;
  /**
   * Returns the secret key of this keypair.
   */
  secretKey(): Uint8Array;
  /**
   * Returns the [PublicKey] of this keypair.
   */
  publicKey(): PublicKey;
}
export class PublicKey {
  private constructor();
  free(): void;
  /**
   * Convert the PublicKey to Uint8Array
   * @deprecated Use `toUint8Array` instead
   */
  to_uint8array(): Uint8Array;
  /**
   * Convert the PublicKey to Uint8Array
   */
  toUint8Array(): Uint8Array;
  /**
   * Returns the z-base32 encoding of this public key
   */
  z32(): string;
  /**
   * @throws
   */
  static from(value: string): PublicKey;
}
export class Session {
  private constructor();
  free(): void;
  /**
   * Return the [PublicKey] of this session
   */
  pubky(): PublicKey;
  /**
   * Return the capabilities that this session has.
   */
  capabilities(): string[];
}
