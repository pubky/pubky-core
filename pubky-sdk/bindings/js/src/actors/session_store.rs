use js_sys::Reflect;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use super::{
    browser_grant_key_store::BrowserGrantKeyStore,
    grant_session::{decode_delegated_grant_state, encode_delegated_grant_state},
    session::Session,
};
use crate::js_error::{JsResult, PubkyError, PubkyErrorName};

const STORE_VERSION: &str = "pubky-session-v1";
const MODE_DELEGATED: &str = "delegated";
const MODE_LOCAL_SECRET: &str = "localSecret";

#[wasm_bindgen(inline_js = r#"
const PUBKY_SESSIONS_DB_NAME = "pubky-auth";
const PUBKY_SESSIONS_DB_VERSION = 1;
const PUBKY_SESSIONS_STORE_NAME = "storedSessions";
const PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME = "delegatedGrantKeys";

function requireIndexedDb() {
  if (!globalThis.indexedDB) {
    throw new Error("Pubky session persistence requires IndexedDB.");
  }
}

function openSessionStoreDb() {
  requireIndexedDb();
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(PUBKY_SESSIONS_DB_NAME, PUBKY_SESSIONS_DB_VERSION);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(PUBKY_SESSIONS_STORE_NAME)) {
        db.createObjectStore(PUBKY_SESSIONS_STORE_NAME, { keyPath: "id" });
      }
      if (!db.objectStoreNames.contains(PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME)) {
        db.createObjectStore(PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME, { keyPath: "keyId" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Opening Pubky session store failed."));
  });
}

async function withSessionStore(mode, fn) {
  const db = await openSessionStoreDb();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(PUBKY_SESSIONS_STORE_NAME, mode);
      const store = tx.objectStore(PUBKY_SESSIONS_STORE_NAME);
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
      tx.onerror = () => reject(tx.error ?? new Error("Pubky session store transaction failed."));
      tx.onabort = () => reject(tx.error ?? new Error("Pubky session store transaction aborted."));
      tx.oncomplete = () => {
        if (!done) resolve(undefined);
      };
    });
  } finally {
    db.close();
  }
}

export async function __pubkySessionStoreIsAvailable() {
  if (!globalThis.indexedDB) return false;
  try {
    const db = await openSessionStoreDb();
    db.close();
    return true;
  } catch (_error) {
    return false;
  }
}

export async function __pubkySessionStorePut(record) {
  requireIndexedDb();
  await withSessionStore("readwrite", (store, resolve, reject) => {
    const request = store.put(record);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error ?? new Error("Saving Pubky session failed."));
  });
}

export async function __pubkySessionStoreGet(id) {
  requireIndexedDb();
  return await withSessionStore("readonly", (store, resolve, reject) => {
    const request = store.get(id);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Reading Pubky session failed."));
  });
}

export async function __pubkySessionStoreList() {
  if (!globalThis.indexedDB) return [];
  try {
    return await withSessionStore("readonly", (store, resolve, reject) => {
      const request = store.getAll();
      request.onsuccess = () => resolve(request.result ?? []);
      request.onerror = () => reject(request.error ?? new Error("Listing Pubky sessions failed."));
    });
  } catch (_error) {
    return [];
  }
}

export async function __pubkySessionStoreDelete(id) {
  requireIndexedDb();
  await withSessionStore("readwrite", (store, resolve, reject) => {
    const request = store.delete(id);
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error ?? new Error("Removing Pubky session failed."));
  });
}

export async function __pubkySessionStoreClear(delegatedKeyIds) {
  requireIndexedDb();
  const db = await openSessionStoreDb();
  try {
    await new Promise((resolve, reject) => {
      const tx = db.transaction(
        [PUBKY_SESSIONS_STORE_NAME, PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME],
        "readwrite",
      );
      const sessions = tx.objectStore(PUBKY_SESSIONS_STORE_NAME);
      const keys = tx.objectStore(PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME);
      sessions.clear();
      for (const keyId of new Set(delegatedKeyIds ?? [])) {
        keys.delete(keyId);
      }
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error ?? new Error("Clearing Pubky session store failed."));
      tx.onabort = () => reject(tx.error ?? new Error("Clearing Pubky session store aborted."));
    });
  } finally {
    db.close();
  }
}

export async function __pubkySessionStoreClearAll() {
  requireIndexedDb();
  const db = await openSessionStoreDb();
  try {
    await new Promise((resolve, reject) => {
      const tx = db.transaction(
        [PUBKY_SESSIONS_STORE_NAME, PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME],
        "readwrite",
      );
      tx.objectStore(PUBKY_SESSIONS_STORE_NAME).clear();
      tx.objectStore(PUBKY_SESSIONS_DELEGATED_KEYS_STORE_NAME).clear();
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error ?? new Error("Clearing Pubky auth store failed."));
      tx.onabort = () => reject(tx.error ?? new Error("Clearing Pubky auth store aborted."));
    });
  } finally {
    db.close();
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = __pubkySessionStoreIsAvailable)]
    fn js_store_is_available() -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStorePut)]
    fn js_store_put(record: JsValue) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStoreGet)]
    fn js_store_get(id: String) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStoreList)]
    fn js_store_list() -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStoreDelete)]
    fn js_store_delete(id: String) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStoreClear)]
    fn js_store_clear(delegated_key_ids: JsValue) -> js_sys::Promise;

    #[wasm_bindgen(js_name = __pubkySessionStoreClearAll)]
    fn js_store_clear_all() -> js_sys::Promise;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSessionRecord {
    version: String,
    id: String,
    storage_mode: String,
    credential: String,
    public_key: String,
    homeserver: String,
    grant_id: String,
    client_id: String,
    capabilities: Vec<String>,
    grant_expires_at: f64,
    created_at: f64,
}

/// Metadata for a session saved in the browser session store.
#[wasm_bindgen]
pub struct StoredSessionInfo(StoredSessionRecord);

#[wasm_bindgen]
impl StoredSessionInfo {
    /// Stable local identifier for this stored session.
    #[wasm_bindgen(getter)]
    pub fn id(&self) -> String {
        self.0.id.clone()
    }

    /// `delegated` for origin-bound WebCrypto sessions, `localSecret` for raw local PoP secret storage.
    #[wasm_bindgen(js_name = "storageMode", getter)]
    pub fn storage_mode(&self) -> String {
        self.0.storage_mode.clone()
    }

    /// User public key as z32.
    #[wasm_bindgen(js_name = "publicKey", getter)]
    pub fn public_key(&self) -> String {
        self.0.public_key.clone()
    }

    /// Homeserver public key as z32.
    #[wasm_bindgen(getter)]
    pub fn homeserver(&self) -> String {
        self.0.homeserver.clone()
    }

    /// Grant identifier backing this stored session.
    #[wasm_bindgen(js_name = "grantId", getter)]
    pub fn grant_id(&self) -> String {
        self.0.grant_id.clone()
    }

    /// Application/client identifier.
    #[wasm_bindgen(js_name = "clientId", getter)]
    pub fn client_id(&self) -> String {
        self.0.client_id.clone()
    }

    /// Authorized capabilities.
    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> Vec<String> {
        self.0.capabilities.clone()
    }

    /// Underlying grant expiry timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "grantExpiresAt", getter)]
    pub fn grant_expires_at(&self) -> f64 {
        self.0.grant_expires_at
    }

    /// Local save timestamp, in Unix milliseconds.
    #[wasm_bindgen(js_name = "createdAt", getter)]
    pub fn created_at(&self) -> f64 {
        self.0.created_at
    }
}

/// Browser-backed durable store for completed grant sessions.
#[wasm_bindgen]
pub struct BrowserSessionStore(pub(crate) pubky::Pubky);

#[wasm_bindgen]
impl BrowserSessionStore {
    /// Whether IndexedDB is available for durable session persistence.
    #[wasm_bindgen(js_name = "isAvailable")]
    pub async fn is_available(&self) -> JsResult<bool> {
        let value = JsFuture::from(js_store_is_available())
            .await
            .map_err(store_error)?;
        Ok(value.as_bool().unwrap_or(false))
    }

    /// Persist a completed grant session in IndexedDB.
    #[wasm_bindgen]
    pub async fn save(&self, session: &Session) -> JsResult<StoredSessionInfo> {
        let grant = session.0.as_grant().ok_or_else(|| {
            PubkyError::new(
                PubkyErrorName::ClientStateError,
                "Only grant-backed sessions can be saved in BrowserSessionStore.",
            )
        })?;
        let session_info = grant.session_info().await;
        let grant_id = session_info.grant_id.to_string();
        let public_key = session_info.pubky.z32();

        let (storage_mode, credential) =
            if let Some(state) = grant.export_delegated_restore_state().await {
                (
                    MODE_DELEGATED.to_string(),
                    encode_delegated_grant_state(state)?,
                )
            } else {
                let secret = grant.export_local_secret().await.ok_or_else(|| {
                    PubkyError::new(
                        PubkyErrorName::ClientStateError,
                        "This grant session cannot export restorable local secret material.",
                    )
                })?;
                (MODE_LOCAL_SECRET.to_string(), secret)
            };

        let record = StoredSessionRecord {
            version: STORE_VERSION.to_string(),
            id: format!("{public_key}:{grant_id}"),
            storage_mode,
            credential,
            public_key,
            homeserver: session_info.homeserver.z32(),
            grant_id,
            client_id: session_info.client_id.to_string(),
            capabilities: session_info
                .capabilities
                .iter()
                .map(ToString::to_string)
                .collect(),
            grant_expires_at: session_info.grant_expires_at as f64,
            created_at: js_sys::Date::now(),
        };

        let value = serde_wasm_bindgen::to_value(&record).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::InternalError,
                format!("Failed to serialize stored session: {e}"),
            )
        })?;
        JsFuture::from(js_store_put(value))
            .await
            .map_err(store_error)?;
        Ok(StoredSessionInfo(record))
    }

    /// List all locally stored sessions for this origin.
    #[wasm_bindgen]
    pub async fn list(&self) -> JsResult<Vec<StoredSessionInfo>> {
        self.stored_records()
            .await
            .map(|records| records.into_iter().map(StoredSessionInfo).collect())
    }

    /// Restore a specific stored session by id.
    #[wasm_bindgen]
    pub async fn restore(&self, id: String) -> JsResult<Session> {
        let record = self.load_record(id).await?;
        match record.storage_mode.as_str() {
            MODE_DELEGATED => {
                let state = decode_delegated_grant_state(&record.credential)?;
                let stored_public_key =
                    BrowserGrantKeyStore::load_public_key(state.key_id.clone()).await?;
                if stored_public_key != state.client_pk {
                    return Err(PubkyError::new(
                        PubkyErrorName::ClientStateError,
                        "Delegated grant key public key does not match saved session.",
                    ));
                }
                let sign = BrowserGrantKeyStore::signer(state.key_id.clone());
                Ok(Session(
                    self.0.restore_delegated_grant_session(state, sign).await?,
                ))
            }
            MODE_LOCAL_SECRET => Ok(Session(self.0.restore_session(&record.credential).await?)),
            _ => Err(PubkyError::new(
                PubkyErrorName::ClientStateError,
                "Unsupported stored session storage mode.",
            )),
        }
    }

    /// Remove local stored session metadata and any SDK-owned delegated key for that record.
    #[wasm_bindgen]
    pub async fn remove(&self, id: String) -> JsResult<()> {
        let record = self.load_record(id.clone()).await?;
        JsFuture::from(js_store_delete(id))
            .await
            .map_err(store_error)?;

        if record.storage_mode == MODE_DELEGATED {
            let state = decode_delegated_grant_state(&record.credential)?;
            BrowserGrantKeyStore::delete_key(state.key_id).await?;
        }

        Ok(())
    }

    /// Clear all local stored session records for this origin.
    ///
    /// Delegated keys referenced by those stored session records are removed.
    /// Delegated keys that only belong to pending grant flows are preserved.
    #[wasm_bindgen]
    pub async fn clear(&self) -> JsResult<()> {
        let records = self.stored_records().await?;
        let delegated_key_ids = delegated_key_ids_for_records(&records)?;
        let delegated_key_ids = serde_wasm_bindgen::to_value(&delegated_key_ids).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::InternalError,
                format!("Failed to serialize delegated key ids: {e}"),
            )
        })?;
        JsFuture::from(js_store_clear(delegated_key_ids))
            .await
            .map_err(store_error)?;
        Ok(())
    }

    /// Clear all browser auth persistence owned by this SDK origin.
    ///
    /// This removes all stored session records and all browser-held delegated
    /// grant keys, including keys for pending delegated grant flows. Saved
    /// delegated flow state becomes unrestorable. This does not revoke remote
    /// grants or immediately invalidate already-live in-memory sessions.
    #[wasm_bindgen(js_name = "clearAll")]
    pub async fn clear_all(&self) -> JsResult<()> {
        JsFuture::from(js_store_clear_all())
            .await
            .map_err(store_error)?;
        Ok(())
    }
}

impl BrowserSessionStore {
    async fn stored_records(&self) -> JsResult<Vec<StoredSessionRecord>> {
        let value = JsFuture::from(js_store_list()).await.map_err(store_error)?;
        let records: Vec<StoredSessionRecord> =
            serde_wasm_bindgen::from_value(value).map_err(|e| {
                PubkyError::new(
                    PubkyErrorName::ClientStateError,
                    format!("Invalid stored session record: {e}"),
                )
            })?;
        records
            .into_iter()
            .map(validate_record)
            .map(|info| info.map(|info| info.0))
            .collect()
    }

    async fn load_record(&self, id: String) -> JsResult<StoredSessionRecord> {
        let value = JsFuture::from(js_store_get(id.clone()))
            .await
            .map_err(store_error)?;
        if value.is_undefined() {
            return Err(PubkyError::new(
                PubkyErrorName::ClientStateError,
                format!("Stored Pubky session not found: {id}"),
            ));
        }
        let record: StoredSessionRecord = serde_wasm_bindgen::from_value(value).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::ClientStateError,
                format!("Invalid stored session record: {e}"),
            )
        })?;
        validate_record(record).map(|info| info.0)
    }
}

fn delegated_key_ids_for_records(records: &[StoredSessionRecord]) -> JsResult<Vec<String>> {
    records
        .iter()
        .filter(|record| record.storage_mode == MODE_DELEGATED)
        .map(|record| decode_delegated_grant_state(&record.credential).map(|state| state.key_id))
        .collect()
}

fn validate_record(record: StoredSessionRecord) -> JsResult<StoredSessionInfo> {
    if record.version != STORE_VERSION {
        return Err(PubkyError::new(
            PubkyErrorName::ClientStateError,
            "Unsupported stored session version.",
        ));
    }
    if record.storage_mode != MODE_DELEGATED && record.storage_mode != MODE_LOCAL_SECRET {
        return Err(PubkyError::new(
            PubkyErrorName::ClientStateError,
            "Unsupported stored session storage mode.",
        ));
    }
    Ok(StoredSessionInfo(record))
}

fn store_error(value: JsValue) -> PubkyError {
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
        .unwrap_or_else(|| "Pubky session store operation failed.".to_string())
}
