//! Tools for encrypting and decrypting a recovery file storing user's root key's secret.

use argon2::Argon2;

use crate::crypto::{decrypt, encrypt, Keypair};

static SPEC_NAME: &str = "recovery";
static SPEC_LINE: &str = "pubky.org/recovery";

/// Decrypt a recovery file.
pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair, Error> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase);

    let newline_index = recovery_file
        .iter()
        .position(|&r| r == 10)
        .ok_or(())
        .map_err(|_| Error::RecoveryFileMissingSpecLine)?;

    let spec_line = &recovery_file[..newline_index];

    if !(spec_line.starts_with(SPEC_LINE.as_bytes())
        || spec_line.starts_with(b"pkarr.org/recovery"))
    {
        return Err(Error::RecoveryFileVersionNotSupported);
    }

    let encrypted = &recovery_file[newline_index + 1..];

    if encrypted.is_empty() {
        return Err(Error::RecoverFileMissingEncryptedSecretKey);
    };

    let decrypted = decrypt(encrypted, &encryption_key)?;
    let length = decrypted.len();
    let secret_key: [u8; 32] = decrypted
        .try_into()
        .map_err(|_| Error::RecoverFileInvalidSecretKeyLength(length))?;

    Ok(Keypair::from_secret_key(&secret_key))
}

/// Encrypt a recovery file.
pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Vec<u8> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase);
    let secret_key = keypair.secret_key();

    let encrypted_secret_key = encrypt(&secret_key, &encryption_key);

    let mut out = Vec::with_capacity(SPEC_LINE.len() + 1 + encrypted_secret_key.len());

    out.extend_from_slice(SPEC_LINE.as_bytes());
    out.extend_from_slice(b"\n");
    out.extend_from_slice(&encrypted_secret_key);

    out
}

fn recovery_file_encryption_key_from_passphrase(passphrase: &str) -> [u8; 32] {
    let argon2id = Argon2::default();

    let mut out = [0; 32];

    argon2id
        .hash_password_into(passphrase.as_bytes(), SPEC_NAME.as_bytes(), &mut out)
        .expect("Output is the correct length, so this should be infallible");

    out
}

#[derive(thiserror::Error, Debug)]
/// Error decrypting a recovery file
pub enum Error {
    // === Recovery file ==
    #[error("Recovery file should start with a spec line, followed by a new line character")]
    /// Recovery file should start with a spec line, followed by a new line character
    RecoveryFileMissingSpecLine,

    #[error("Recovery file should start with a spec line, followed by a new line character")]
    /// Recovery file should start with a spec line, followed by a new line character
    RecoveryFileVersionNotSupported,

    #[error("Recovery file should contain an encrypted secret key after the new line character")]
    /// Recovery file should contain an encrypted secret key after the new line character
    RecoverFileMissingEncryptedSecretKey,

    #[error("Recovery file encrypted secret key should be 32 bytes, got {0}")]
    /// Recovery file encrypted secret key should be 32 bytes, got {0}
    RecoverFileInvalidSecretKeyLength(usize),

    #[error(transparent)]
    /// Error while decrypting a message
    DecryptError(#[from] crate::crypto::DecryptError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_recovery_file() {
        let passphrase = "very secure password";
        let keypair = Keypair::random();

        let recovery_file = create_recovery_file(&keypair, passphrase);
        let recovered = decrypt_recovery_file(&recovery_file, passphrase).unwrap();

        assert_eq!(recovered.public_key(), keypair.public_key());
    }
}
