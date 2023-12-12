//! Encryption functions.

use crate::{Error, Result};

use crate::crypto::Key;

/// Compute the length of a ciphertext, given the length of a plaintext.
///
/// This function returns `None` if the resulting ciphertext length would overflow a `u64`.
pub fn ciphertext_len(plaintext_len: u64) -> Option<u64> {
    bessie::ciphertext_len(plaintext_len)
}

/// Encrypt a message and write the ciphertext to an existing slice.
///
/// This function does not allocate memory. However, `ciphertext.len()` must be exactly equal to
/// [`ciphertext_len(plaintext.len())`](ciphertext_len), or else this function will panic.
pub fn encrypt_to_slice(key: &Key, plaintext: &[u8], ciphertext: &mut [u8]) {
    bessie::encrypt_to_slice(key, plaintext, ciphertext)
}

/// Encrypt a message and return the ciphertext as a `Vec<u8>`.
pub fn encrypt(key: &Key, plaintext: &[u8]) -> Vec<u8> {
    bessie::encrypt(key, plaintext)
}

/// Decrypt a message and return the plaintext as `Result` of `Vec<u8>`.
///
/// If the ciphertext or key has been changed, decryption will return `Err`.
pub fn decrypt(key: &Key, ciphertext: &[u8]) -> Result<Vec<u8>> {
    bessie::decrypt(key, ciphertext).map_err(|err| Error::Generic(err.to_string()))
}
