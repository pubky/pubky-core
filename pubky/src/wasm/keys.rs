use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Keypair(pkarr::Keypair);

#[wasm_bindgen]
impl Keypair {
    #[wasm_bindgen]
    /// Generate a random [Keypair]
    pub fn random(secret_key: js_sys::Uint8Array) -> Self {
        Self(pkarr::Keypair::random())
    }

    #[wasm_bindgen]
    /// Generate a [Keypair] from a secret key.
    pub fn fromSecretKey(secret_key: js_sys::Uint8Array) -> Self {
        let mut bytes = [0; 32];
        secret_key.copy_to(&mut bytes);

        Self(pkarr::Keypair::from_secret_key(&bytes))
    }

    #[wasm_bindgen]
    /// Returns the [PublicKey] of this keypair.
    pub fn publicKey(&self) -> PublicKey {
        PublicKey(self.0.public_key())
    }
}

#[wasm_bindgen]
pub struct PublicKey(pkarr::PublicKey);

#[wasm_bindgen]
impl PublicKey {
    #[wasm_bindgen]
    /// Return the public key as Uint8Array
    pub fn toBytes(&self) -> js_sys::Uint8Array {
        js_sys::Uint8Array::from(self.0.as_bytes().as_slice())
    }

    #[wasm_bindgen]
    pub fn toString(&self) -> String {
        self.0.to_string()
    }
}
