use crypto_secretbox::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    XSalsa20Poly1305,
};
use rand::random;

pub use pkarr::{Keypair, PublicKey};

pub use ed25519_dalek::Signature;

pub type Hash = blake3::Hash;

pub use blake3::hash;

pub use blake3::Hasher;

pub fn random_hash() -> Hash {
    Hash::from_bytes(random())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let arr: [u8; N] = random();

    arr
}

pub fn encrypt(plain_text: &[u8], encryption_key: &[u8; 32]) -> Result<Vec<u8>, EncryptError> {
    let cipher = XSalsa20Poly1305::new(encryption_key.into());
    let nonce = XSalsa20Poly1305::generate_nonce(&mut OsRng); // unique per message
    let ciphertext = cipher.encrypt(&nonce, plain_text)?;

    let mut out: Vec<u8> = Vec::with_capacity(nonce.len() + ciphertext.len());
    out.extend_from_slice(nonce.as_slice());
    out.extend_from_slice(&ciphertext);

    Ok(out)
}

pub fn decrypt(bytes: &[u8], encryption_key: &[u8; 32]) -> Result<Vec<u8>, DecryptError> {
    let cipher = XSalsa20Poly1305::new(encryption_key.into());

    if bytes.len() < 24 {
        return Err(DecryptError::PayloadTooSmall(bytes.len()));
    }

    Ok(cipher.decrypt(bytes[..24].into(), &bytes[24..])?)
}

#[derive(thiserror::Error, Debug)]
pub enum EncryptError {
    #[error(transparent)]
    SecretBox(#[from] crypto_secretbox::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum DecryptError {
    #[error(transparent)]
    SecretBox(#[from] crypto_secretbox::Error),

    #[error("Encrypted message too small, expected at least 24 bytes nonce, receieved {0} bytes")]
    PayloadTooSmall(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt() {
        let plain_text = "Plain text!";
        let encryption_key = [0; 32];

        let encrypted = encrypt(plain_text.as_bytes(), &encryption_key).unwrap();
        let decrypted = decrypt(&encrypted, &encryption_key).unwrap();

        assert_eq!(decrypted, plain_text.as_bytes())
    }
}
