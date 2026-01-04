//! Cryptographic functions (hashing, encryption, and signatures).

use crypto_secretbox::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XSalsa20Poly1305,
};
use rand::random;

mod keys;
pub use keys::{is_prefixed_pubky, Keypair, PublicKey};

pub use ed25519_dalek::Signature;

/// Blake3 Hash.
pub type Hash = blake3::Hash;

pub use blake3::hash;

pub use blake3::Hasher;

/// Create a random hash.
pub fn random_hash() -> Hash {
    Hash::from_bytes(random())
}

/// Create an array of random bytes with a size `N`.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let arr: [u8; N] = random();

    arr
}

/// Encrypt a message using `XSalsa20Poly1305`.
pub fn encrypt(plain_text: &[u8], encryption_key: &[u8; 32]) -> Vec<u8> {
    if plain_text.is_empty() {
        return plain_text.to_vec();
    }

    let cipher = XSalsa20Poly1305::new(encryption_key.into());
    let nonce = XSalsa20Poly1305::generate_nonce(&mut OsRng); // unique per message
    let ciphertext = cipher
        .encrypt(&nonce, plain_text)
        .expect("XSalsa20Poly1305 encrypt should be infallible");

    let mut out: Vec<u8> = Vec::with_capacity(nonce.len() + ciphertext.len());
    out.extend_from_slice(nonce.as_ref());
    out.extend_from_slice(&ciphertext);

    out
}

/// Encrypt an encrypted message using `XSalsa20Poly1305`.
pub fn decrypt(bytes: &[u8], encryption_key: &[u8; 32]) -> Result<Vec<u8>, DecryptError> {
    if bytes.is_empty() {
        return Ok(bytes.to_vec());
    }

    let cipher = XSalsa20Poly1305::new(encryption_key.into());

    if bytes.len() < 24 {
        return Err(DecryptError::TooSmall(bytes.len()));
    }

    Ok(cipher.decrypt(bytes[..24].into(), &bytes[24..])?)
}

#[derive(thiserror::Error, Debug)]
/// Error while decrypting a message
pub enum DecryptError {
    #[error(transparent)]
    /// Failed to decrypt message.
    Fail(#[from] crypto_secretbox::Error),

    #[error("Encrypted message too small, expected at least 24 bytes nonce, received {0} bytes")]
    /// Encrypted message too small, expected at least 24 bytes nonce, received {0} bytes
    TooSmall(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        let plain_text = "Plain text!";
        let encryption_key = [0; 32];

        let encrypted = encrypt(plain_text.as_bytes(), &encryption_key);
        let decrypted = decrypt(&encrypted, &encryption_key).unwrap();

        assert_eq!(decrypted, plain_text.as_bytes())
    }
}
