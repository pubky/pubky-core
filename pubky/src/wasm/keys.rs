use wasm_bindgen::prelude::*;

use crate::Error;

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
    pub fn from_secret_key(secret_key: js_sys::Uint8Array) -> Result<Keypair, JsValue> {
        if !js_sys::Uint8Array::instanceof(&secret_key) {
            return Err("Expected secret_key to be an instance of Uint8Array".into());
        }

        let len = secret_key.byte_length();
        if len != 32 {
            return Err(format!("Expected secret_key to be 32 bytes, got {len}"))?;
        }

        let mut bytes = [0; 32];
        secret_key.copy_to(&mut bytes);

        Ok(Self(pkarr::Keypair::from_secret_key(&bytes)))
    }

    /// Returns the secret key of this keypair.
    #[wasm_bindgen(js_name = "secretKey")]
    pub fn secret_key(&self) -> js_sys::Uint8Array {
        self.0.secret_key().as_slice().into()
    }

    /// Returns the [PublicKey] of this keypair.
    #[wasm_bindgen(js_name = "publicKey")]
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
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
    #[wasm_bindgen]
    /// Convert the PublicKey to Uint8Array
    pub fn to_uint8array(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.0.as_bytes().as_slice())
    }

    #[wasm_bindgen]
    /// Returns the z-base32 encoding of this public key
    pub fn z32(&self) -> String {
        self.0.to_string()
    }

    #[wasm_bindgen(js_name = "from")]
    /// @throws
    pub fn try_from(value: JsValue) -> Result<PublicKey, JsValue> {
        let string = value
            .as_string()
            .ok_or("Couldn't create a PublicKey from this type of value")?;

        Ok(PublicKey(
            pkarr::PublicKey::try_from(string).map_err(Error::Pkarr)?,
        ))
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
