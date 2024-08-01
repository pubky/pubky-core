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

    #[wasm_bindgen]
    /// Generate a [Keypair] from a secret key.
    pub fn from_secret_key(secret_key: js_sys::Uint8Array) -> Self {
        let mut bytes = [0; 32];
        secret_key.copy_to(&mut bytes);

        Self(pkarr::Keypair::from_secret_key(&bytes))
    }

    #[wasm_bindgen(js_name = "publicKey")]
    /// Returns the [PublicKey] of this keypair.
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }
}

impl Keypair {
    pub fn as_inner(&self) -> &pkarr::Keypair {
        &self.0
    }
}

#[wasm_bindgen]
pub struct PublicKey(pkarr::PublicKey);

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
        let string = value.as_string().ok_or(Error::Generic(
            "Couldn't create a PublicKey from this type of value".to_string(),
        ))?;

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
