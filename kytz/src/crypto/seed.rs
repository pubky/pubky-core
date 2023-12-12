//! Manage Kytz seed files.
//!
//! Seed file contains a seed encrypted with a strong passphrase.

use bytes::{Bytes, BytesMut};
use zeroize::Zeroize;

use crate::{
    crypto::{
        encryption::{ciphertext_len, decrypt, encrypt_to_slice},
        Key,
    },
    Error, Result,
};

const SEED_SCHEME: &[u8] = b"kytz:seed:";

const VERSION: u8 = 0;

/// Encrypt the seed with a strong passphrase, and return an [encrypted seed
/// file](../../../design/seed.md).
pub fn encrypt_seed(seed: &Key, passphrase: &str) -> Bytes {
    let mut encryption_key = derive_encryption_key(passphrase);

    let mut seed_file = BytesMut::with_capacity(SEED_SCHEME.len() + 33);
    seed_file.extend_from_slice(SEED_SCHEME);

    let suffix_len = 1 + ciphertext_len(seed.len() as u64).unwrap() as usize;
    let mut suffix = vec![0_u8; suffix_len];

    suffix[0] = VERSION;
    encrypt_to_slice(&encryption_key, seed, &mut suffix[1..]);

    seed_file.extend_from_slice(z32::encode(&suffix).as_bytes());

    encryption_key.zeroize();

    seed_file.freeze()
}

/// Decrypt the [seed file](../../../design/seed.md).
pub fn decrypt_seed(seed_file: Bytes, passphrase: &str) -> Result<Vec<u8>> {
    if !seed_file.starts_with(SEED_SCHEME) {
        return Err(Error::Generic("Not a Kytz seed".to_string()));
    }

    let suffix = z32::decode(&seed_file[SEED_SCHEME.len()..])
        .map_err(|_| Error::Generic("Invalid seed encoding".to_string()))?;

    let version = suffix[0];

    match version {
        0 => decrypted_seed_v0(&suffix, passphrase),
        _ => Err(Error::Generic("Unknown Kytz seed file version".to_string())),
    }
}

fn decrypted_seed_v0(suffix: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let mut encryption_key = derive_encryption_key(passphrase);
    let encrypted_seed = &suffix[1..];

    let decrypted_seed = decrypt(&encryption_key, encrypted_seed);

    // Empty the encryption key in memory.
    encryption_key.zeroize();

    decrypted_seed
}

/// Derive a secret key from a strong passphrase for encrypting/decrypting the seed.
fn derive_encryption_key(passphrase: &str) -> Key {
    // Argon2 with default params (Argon2id v19)
    let hasher = argon2::Argon2::default();

    let mut encryption_key: Key = [0; 32];

    hasher
        .hash_password_into(
            passphrase.as_bytes(),
            // While this is technically a Nonce reuse, it should not be a problem
            // since the encryption key is never shared or stored anywhere.
            SEED_SCHEME,
            &mut encryption_key,
        )
        // There shouldn't be any error, as we use the default params.
        .unwrap();

    encryption_key
}

#[cfg(test)]
mod test {
    use std::time::Instant;

    use super::*;
    use crate::crypto::passphrase::*;
    use crate::crypto::*;

    #[test]
    fn test_encrypt_decrypt_seed() {
        let seed = generate_seed();
        let passphrase = generate_4words_passphrase();

        let encrypted_seed_file = encrypt_seed(&seed, &passphrase);

        // dbg!(&encrypted_seed_file);

        let start = Instant::now();
        let decrypted_seed = decrypt_seed(encrypted_seed_file, &passphrase)
            .expect("Failde to decrypt the seed file");

        assert!(
            start.elapsed().as_millis() > 300,
            "decrypting the seed shouldn't be too fast"
        );
        assert_eq!(decrypted_seed, seed);
    }
}
