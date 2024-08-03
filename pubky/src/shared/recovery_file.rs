use argon2::Argon2;
use pkarr::Keypair;
use pubky_common::crypto::{decrypt, encrypt};

use crate::{
    error::{Error, Result},
    PubkyClient,
};

static SPEC_NAME: &str = "recovery";
static SPEC_LINE: &str = "pubky.org/recovery";

pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase)?;

    let mut split = recovery_file.split(|byte| byte == &10);

    match split.next() {
        Some(bytes) => {
            if !(bytes.starts_with(SPEC_LINE.as_bytes())
                || bytes.starts_with(b"pkarr.org/recovery"))
            {
                return Err(Error::RecoveryFileVersionNotSupported);
            }
        }
        None => return Err(Error::RecoveryFileMissingSpecLine),
    };

    if let Some(encrypted) = split.next() {
        let decrypted = decrypt(encrypted, &encryption_key)?;
        let length = decrypted.len();
        let secret_key: [u8; 32] = decrypted
            .try_into()
            .map_err(|_| Error::RecoverFileInvalidSecretKeyLength(length))?;

        return Ok(Keypair::from_secret_key(&secret_key));
    };

    Err(Error::RecoverFileMissingEncryptedSecretKey)
}

pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Result<Vec<u8>> {
    let encryption_key = recovery_file_encryption_key_from_passphrase(passphrase)?;
    let secret_key = keypair.secret_key();

    let encrypted_secret_key = encrypt(&secret_key, &encryption_key)?;

    let mut out = Vec::with_capacity(SPEC_LINE.len() + 1 + encrypted_secret_key.len());

    out.extend_from_slice(SPEC_LINE.as_bytes());
    out.extend_from_slice(b"\n");
    out.extend_from_slice(&encrypted_secret_key);

    Ok(out)
}

fn recovery_file_encryption_key_from_passphrase(passphrase: &str) -> Result<[u8; 32]> {
    let argon2id = Argon2::default();

    let mut out = [0; 32];

    argon2id.hash_password_into(passphrase.as_bytes(), SPEC_NAME.as_bytes(), &mut out)?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_recovery_file() {
        let passphrase = "very secure password";
        let keypair = Keypair::random();

        let recovery_file = PubkyClient::create_recovery_file(&keypair, passphrase).unwrap();
        let recovered = PubkyClient::decrypt_recovery_file(&recovery_file, passphrase).unwrap();

        assert_eq!(recovered.public_key(), keypair.public_key());
    }
}
