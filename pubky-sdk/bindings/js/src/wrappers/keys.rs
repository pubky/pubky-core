use wasm_bindgen::prelude::*;

use crate::js_error::JsResult;
use js_sys::Uint8Array;

#[wasm_bindgen]
pub struct Keypair(pkarr::Keypair);

#[wasm_bindgen]
impl Keypair {
    #[wasm_bindgen]
    /// Generate a random [Keypair]
    pub fn random() -> Self {
        Self(pkarr::Keypair::random())
    }

    /// Generate a [Keypair] from a secret key.
    #[wasm_bindgen(js_name = "fromSecretKey")]
    pub fn from_secret_key(secret_key: Vec<u8>) -> Result<Keypair, JsValue> {
        let secret_len = secret_key.len();
        let secret: [u8; 32] = secret_key
            .try_into()
            .map_err(|_| format!("Expected secret_key to be 32 bytes, got {}", secret_len))?;
        Ok(Self(pkarr::Keypair::from_secret_key(&secret)))
    }

    /// Returns the secret key of this keypair.
    #[wasm_bindgen(js_name = "secretKey")]
    pub fn secret_key(&self) -> Uint8Array {
        Uint8Array::from(self.0.secret_key().as_ref())
    }

    /// Returns the [PublicKey] of this keypair.
    #[wasm_bindgen(js_name = "publicKey")]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }

    /// Create a recovery file for this keypair (encrypted with the given passphrase).
    #[wasm_bindgen(js_name = "createRecoveryFile")]
    pub fn create_recovery_file(&self, passphrase: &str) -> Uint8Array {
        pubky_common::recovery_file::create_recovery_file(self.as_inner(), passphrase)
            .as_slice()
            .into()
    }

    /// Decrypt a recovery file and return a Keypair (decrypted with the given passphrase).
    #[wasm_bindgen(js_name = "fromRecoveryFile")]
    pub fn from_recovery_file(recovery_file: &[u8], passphrase: &str) -> JsResult<Keypair> {
        let keypair =
            pubky_common::recovery_file::decrypt_recovery_file(recovery_file, passphrase)?;
        Ok(Keypair::from(keypair))
    }
}

impl Keypair {
    pub fn as_inner(&self) -> &pkarr::Keypair {
        &self.0
    }
}

impl From<pkarr::Keypair> for Keypair {
    fn from(keypair: pkarr::Keypair) -> Self {
        Self(keypair)
    }
}

#[wasm_bindgen]
pub struct PublicKey(pub(crate) pkarr::PublicKey);

#[wasm_bindgen]
impl PublicKey {
    /// Convert the PublicKey to Uint8Array
    #[wasm_bindgen(js_name = "toUint8Array")]
    pub fn to_uint8array(&self) -> Uint8Array {
        Uint8Array::from(self.0.as_bytes().as_ref())
    }

    #[wasm_bindgen]
    /// Returns the z-base32 encoding of this public key
    pub fn z32(&self) -> String {
        self.0.to_string()
    }

    #[wasm_bindgen(js_name = "from")]
    /// @throws
    pub fn try_from(value: String) -> JsResult<PublicKey> {
        let native_pk = pkarr::PublicKey::try_from(value)?;
        Ok(PublicKey(native_pk))
    }
}

impl PublicKey {
    pub fn as_inner(&self) -> &pkarr::PublicKey {
        &self.0
    }
}

impl From<pkarr::PublicKey> for PublicKey {
    fn from(value: pkarr::PublicKey) -> Self {
        PublicKey(value)
    }
}
