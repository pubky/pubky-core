use argon2::Argon2;
use pkarr::Keypair;

use crate::crypto::{decrypt, encrypt};

static SPEC_NAME: &str = "recovery";
static SPEC_LINE: &str = "pubky.org/recovery";

pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair, Error> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase)?;

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

pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Result<Vec<u8>, Error> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase)?;
    let secret_key = keypair.secret_key();

    let encrypted_secret_key = encrypt(&secret_key, &encryption_key)?;

    let mut out = Vec::with_capacity(SPEC_LINE.len() + 1 + encrypted_secret_key.len());

    out.extend_from_slice(SPEC_LINE.as_bytes());
    out.extend_from_slice(b"\n");
    out.extend_from_slice(&encrypted_secret_key);

    Ok(out)
}

fn recovery_file_encryption_key_from_passphrase(passphrase: &str) -> Result<[u8; 32], Error> {
    let argon2id = Argon2::default();

    let mut out = [0; 32];

    argon2id.hash_password_into(passphrase.as_bytes(), SPEC_NAME.as_bytes(), &mut out)?;

    Ok(out)
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    // === Recovery file ==
    #[error("Recovery file should start with a spec line, followed by a new line character")]
    RecoveryFileMissingSpecLine,

    #[error("Recovery file should start with a spec line, followed by a new line character")]
    RecoveryFileVersionNotSupported,

    #[error("Recovery file should contain an encrypted secret key after the new line character")]
    RecoverFileMissingEncryptedSecretKey,

    #[error("Recovery file encrypted secret key should be 32 bytes, got {0}")]
    RecoverFileInvalidSecretKeyLength(usize),

    #[error(transparent)]
    Argon(#[from] argon2::Error),

    #[error(transparent)]
    Crypto(#[from] crate::crypto::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_recovery_file() {
        let passphrase = "very secure password";
        let keypair = Keypair::random();

        let recovery_file = create_recovery_file(&keypair, passphrase).unwrap();
        let recovered = decrypt_recovery_file(&recovery_file, passphrase).unwrap();

        assert_eq!(recovered.public_key(), keypair.public_key());
    }
}
