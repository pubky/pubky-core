//! Browser-backed grant PoP key storage and signing.
//!
//! This module is the JS/WASM binding layer for non-extractable grant signing
//! keys. Rust SDK core owns the grant auth state machine and accepts an async
//! signing callback, while this module creates, stores, restores, and uses the
//! browser WebCrypto/IndexedDB key handles needed to implement that callback.

use js_sys::{Reflect, Uint8Array};
use pubky::{PublicKey as NativePublicKey, delegated_sign_callback};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::js_error::{JsResult, PubkyError, PubkyErrorName};

#[wasm_bindgen(inline_js = r#"
const PUBKY_GRANT_KEYS_DB_NAME = "pubky-auth";
const PUBKY_GRANT_KEYS_DB_VERSION = 1;
const PUBKY_GRANT_KEYS_STORE_NAME = "delegatedGrantKeys";
const PUBKY_GRANT_KEYS_SESSION_STORE_NAME = "storedSessions";
let canUseDelegationPromise;
const TEST_CAN_USE_DELEGATION_OVERRIDE = "__pubkyGrantCanUseDelegationOverride";

/**
 * Assert that this runtime can persist non-extractable browser signing keys.
 *
 * SubtleCrypto is exposed only in secure contexts. We rely on browser feature
 * detection here so each browser can decide which origins are trustworthy,
 * including localhost, file://, extension pages, and workers.
 */
function requireBrowserCrypto() {
  const g = globalThis;

  if (!g.crypto?.subtle || !g.indexedDB) {
    if (!g.isSecureContext) {
      throw new Error("Delegated grant keys require a secure browser context.");
    }
    throw new Error("Delegated grant keys require browser WebCrypto and IndexedDB.");
  }
}

/**
 * Return whether this runtime appears able to use delegated grant keys.
 *
 * This is a synchronous feature check only. Starting, resuming, or restoring a
 * delegated flow can still fail later if IndexedDB access is denied, storage is
 * cleared, or a saved key id no longer exists.
 */
export function __pubkyGrantIsDelegationAvailable() {
  const g = globalThis;
  return Boolean(g.isSecureContext && g.crypto?.subtle && g.indexedDB);
}

/**
 * Return whether this runtime can actually create, store, and use browser-held
 * Ed25519 delegated grant keys.
 *
 * Some browser-like runtimes expose secure WebCrypto and IndexedDB but do not
 * implement Ed25519. Cache the probe because it creates a real CryptoKey and
 * verifies that IndexedDB can store/delete it.
 */
export async function __pubkyGrantCanUseDelegation() {
  const override = globalThis[TEST_CAN_USE_DELEGATION_OVERRIDE];
  if (override === true || override === false) return override;

  if (!canUseDelegationPromise) {
    canUseDelegationPromise = (async () => {
      try {
        requireBrowserCrypto();
        const keyId = `__pubky_probe_${randomKeyId()}`;
        const pair = await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"]);
        const publicKeyRaw = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
        const data = new TextEncoder().encode("pubky-delegated-grant-probe");
        const signature = new Uint8Array(await crypto.subtle.sign({ name: "Ed25519" }, pair.privateKey, data));
        if (publicKeyRaw.length !== 32 || signature.length !== 64) return false;
        await putRecord({
          keyId,
          privateKey: pair.privateKey,
          publicKeyRaw,
          createdAt: Date.now(),
        });
        await deleteRecord(keyId);
        return true;
      } catch (_error) {
        return false;
      }
    })();
  }
  return await canUseDelegationPromise;
}

/**
 * Open the IndexedDB database used for delegated grant keys.
 *
 * Records are keyed by the SDK-level key id and contain a non-extractable
 * CryptoKey plus the raw public key bytes needed for grant metadata.
 */
function openGrantKeyDb() {
  requireBrowserCrypto();
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(PUBKY_GRANT_KEYS_DB_NAME, PUBKY_GRANT_KEYS_DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(PUBKY_GRANT_KEYS_STORE_NAME)) {
        db.createObjectStore(PUBKY_GRANT_KEYS_STORE_NAME, { keyPath: "keyId" });
      }
      if (!db.objectStoreNames.contains(PUBKY_GRANT_KEYS_SESSION_STORE_NAME)) {
        db.createObjectStore(PUBKY_GRANT_KEYS_SESSION_STORE_NAME, { keyPath: "id" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Opening delegated grant key store failed."));
  });
}

/**
 * Run an IndexedDB operation against the delegated-key object store.
 *
 * The callback receives the object store plus resolve/reject helpers. This
 * wrapper centralizes transaction lifecycle handling and always closes the DB
 * connection after the operation settles.
 */
async function withGrantKeyStore(mode, fn) {
  const db = await openGrantKeyDb();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(PUBKY_GRANT_KEYS_STORE_NAME, mode);
      const store = tx.objectStore(PUBKY_GRANT_KEYS_STORE_NAME);
      let done = false;
      function finish(value) {
        done = true;
        resolve(value);
      }
      try {
        fn(store, finish, reject);
      } catch (error) {
        reject(error);
      }
      tx.onerror = () => reject(tx.error ?? new Error("Delegated grant key transaction failed."));
      tx.onabort = () => reject(tx.error ?? new Error("Delegated grant key transaction aborted."));
      tx.oncomplete = () => {
        if (!done) resolve(undefined);
      };
    });
  } finally {
    db.close();
  }
}

/**
 * Load a delegated-key record by key id.
 *
 * Returns `undefined` when the key id is unknown. Callers decide whether that
 * should create a new key or surface a restore/signing error.
 */
async function getRecord(keyId) {
  return await withGrantKeyStore("readonly", (store, resolve, reject) => {
    const request = store.get(keyId);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Reading delegated grant key failed."));
  });
}

/**
 * Persist a delegated-key record.
 *
 * The private key is a non-extractable CryptoKey, so IndexedDB stores a browser
 * handle rather than raw private key bytes.
 */
async function putRecord(record) {
  await withGrantKeyStore("readwrite", (store, resolve, reject) => {
    const request = store.put(record);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error ?? new Error("Saving delegated grant key failed."));
  });
}

async function deleteRecord(keyId) {
  await withGrantKeyStore("readwrite", (store, resolve, reject) => {
    const request = store.delete(keyId);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error ?? new Error("Deleting delegated grant key failed."));
  });
}

/**
 * Generate an opaque key id for the IndexedDB record.
 *
 * Use randomUUID when available, with a getRandomValues fallback for older
 * secure browser contexts.
 */
function randomKeyId() {
  if (globalThis.crypto?.randomUUID) return crypto.randomUUID();
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Ensure a delegated grant signing key exists.
 *
 * If `keyId` is provided by an internal restore/reuse path and already exists,
 * return its public key. Otherwise generate a new non-extractable Ed25519
 * keypair, store the private CryptoKey in IndexedDB, and return
 * `{ keyId, publicKey }`.
 */
export async function __pubkyGrantEnsureDelegatedKey(keyId) {
  requireBrowserCrypto();
  const resolvedKeyId = keyId || randomKeyId();
  const existing = await getRecord(resolvedKeyId);
  if (existing) {
    return { keyId: resolvedKeyId, publicKey: new Uint8Array(existing.publicKeyRaw) };
  }

  const pair = await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"]);
  const publicKeyRaw = new Uint8Array(await crypto.subtle.exportKey("raw", pair.publicKey));
  await putRecord({
    keyId: resolvedKeyId,
    privateKey: pair.privateKey,
    publicKeyRaw,
    createdAt: Date.now(),
  });
  return { keyId: resolvedKeyId, publicKey: publicKeyRaw };
}

/**
 * Load the public key bytes for a delegated grant key.
 *
 * Used during delegated session restore to verify that saved metadata still
 * matches the browser-held key.
 */
export async function __pubkyGrantLoadDelegatedPublicKey(keyId) {
  requireBrowserCrypto();
  const existing = await getRecord(keyId);
  if (!existing) throw new Error(`Delegated grant key not found: ${keyId}`);
  return new Uint8Array(existing.publicKeyRaw);
}

/**
 * Sign a grant PoP JWS signing input with a delegated browser key.
 *
 * `signingInput` is the exact ASCII JWS signing input produced by Rust:
 * `base64url(header) + "." + base64url(claims)`. The raw Ed25519 signature
 * bytes are returned to Rust, which finishes compact JWS serialization.
 */
export async function __pubkyGrantDelegatedSign(keyId, signingInput) {
  requireBrowserCrypto();
  const existing = await getRecord(keyId);
  if (!existing?.privateKey) throw new Error(`Delegated grant key not found: ${keyId}`);
  const data = new TextEncoder().encode(signingInput);
  return new Uint8Array(await crypto.subtle.sign({ name: "Ed25519" }, existing.privateKey, data));
}

/**
 * Delete a browser-held delegated grant key by id.
 *
 * Used when a persisted delegated session record is removed from the SDK
 * session store. Missing keys are treated as already deleted.
 */
export async function __pubkyGrantDeleteDelegatedKey(keyId) {
  requireBrowserCrypto();
  await deleteRecord(keyId);
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = __pubkyGrantIsDelegationAvailable)]
    fn js_is_delegation_available() -> bool;

    #[wasm_bindgen(js_name = __pubkyGrantCanUseDelegation)]
    fn js_can_use_delegation() -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkyGrantEnsureDelegatedKey)]
    fn js_ensure_delegated_key(key_id: Option<String>) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkyGrantLoadDelegatedPublicKey)]
    fn js_load_delegated_public_key(key_id: String) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkyGrantDelegatedSign)]
    fn js_delegated_sign(key_id: String, signing_input: String) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkyGrantDeleteDelegatedKey)]
    fn js_delete_delegated_key(key_id: String) -> js_sys::Promise;
}

/// Stateless namespace for browser-held delegated grant PoP keys.
pub(crate) struct BrowserGrantKeyStore;

impl BrowserGrantKeyStore {
    /// Return whether the current JS runtime supports browser-held delegated grant keys.
    ///
    /// This checks for a secure browser context with WebCrypto `crypto.subtle` and
    /// IndexedDB. It does not prove that a later IndexedDB operation will succeed.
    pub(crate) fn is_available() -> bool {
        js_is_delegation_available()
    }

    /// Return whether this runtime can create, persist, and use browser-held Ed25519 keys.
    pub(crate) async fn can_use_delegation() -> bool {
        match JsFuture::from(js_can_use_delegation()).await {
            Ok(value) => value.as_bool().unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Ensure a browser-held delegated grant key exists and return its key id and public key.
    ///
    /// If `key_id` is `Some`, an internal restore/reuse path can reuse an existing
    /// IndexedDB record when present; otherwise a new non-extractable WebCrypto
    /// Ed25519 key is generated and stored.
    pub(crate) async fn ensure_key(key_id: Option<String>) -> JsResult<(String, NativePublicKey)> {
        let value = JsFuture::from(js_ensure_delegated_key(key_id))
            .await
            .map_err(js_error)?;
        let key_id = Reflect::get(&value, &JsValue::from_str("keyId"))
            .map_err(js_error)?
            .as_string()
            .ok_or_else(|| {
                PubkyError::new(
                    PubkyErrorName::ClientStateError,
                    "Delegated grant key store returned an invalid key id.",
                )
            })?;
        let public_key = Reflect::get(&value, &JsValue::from_str("publicKey")).map_err(js_error)?;
        Ok((key_id, public_key_from_js(public_key)?))
    }

    /// Load the public key for an existing browser-held delegated grant key.
    ///
    /// Used during delegated restore to verify that saved grant state still points
    /// at the same non-extractable key in IndexedDB.
    pub(crate) async fn load_public_key(key_id: String) -> JsResult<NativePublicKey> {
        let value = JsFuture::from(js_load_delegated_public_key(key_id))
            .await
            .map_err(js_error)?;
        public_key_from_js(value)
    }

    /// Build the Rust delegated signing callback for a browser-held grant key.
    ///
    /// On wasm, the callback forwards each JWS signing input to WebCrypto and
    /// returns raw Ed25519 signature bytes. On non-wasm targets, this returns a
    /// callback that fails with an unsupported-runtime error so native checks still
    /// compile.
    pub(crate) fn signer(key_id: String) -> pubky::DelegatedSignFn {
        #[cfg(target_arch = "wasm32")]
        {
            delegated_sign_callback(move |signing_input| {
                let key_id = key_id.clone();
                async move {
                    let value = JsFuture::from(js_delegated_sign(key_id, signing_input))
                        .await
                        .map_err(|value| {
                            pubky::Error::Authentication(pubky::errors::AuthError::Validation(
                                js_error_message(value),
                            ))
                        })?;
                    Ok(Uint8Array::new(&value).to_vec())
                }
            })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = key_id;
            return delegated_sign_callback(|_| async {
                Err(pubky::Error::Authentication(
                    pubky::errors::AuthError::Validation(
                        "Delegated grant signing is only available in wasm browser builds.".into(),
                    ),
                ))
            });
        }
    }

    /// Delete a browser-held delegated grant key. Missing keys are ignored by IndexedDB.
    pub(crate) async fn delete_key(key_id: String) -> JsResult<()> {
        JsFuture::from(js_delete_delegated_key(key_id))
            .await
            .map_err(js_error)?;
        Ok(())
    }
}

fn public_key_from_js(value: JsValue) -> JsResult<NativePublicKey> {
    let bytes = Uint8Array::new(&value).to_vec();
    let raw: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        PubkyError::new(
            PubkyErrorName::ClientStateError,
            "Delegated grant key store returned an invalid public key.",
        )
    })?;
    pkarr::PublicKey::try_from(&raw)
        .map(NativePublicKey::from)
        .map_err(|err| PubkyError::new(PubkyErrorName::ClientStateError, err))
}

fn js_error(value: JsValue) -> PubkyError {
    PubkyError::new(PubkyErrorName::ClientStateError, js_error_message(value))
}

fn js_error_message(value: JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            Reflect::get(&value, &JsValue::from_str("message"))
                .ok()
                .and_then(|value| value.as_string())
        })
        .unwrap_or_else(|| "Delegated grant key operation failed.".to_string())
}
