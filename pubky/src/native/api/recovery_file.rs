use pubky_common::{
    crypto::Keypair,
    recovery_file::{create_recovery_file, decrypt_recovery_file},
};

use anyhow::Result;

use crate::Client;

impl Client {
    /// Create a recovery file of the `keypair`, containing the secret key encrypted
    /// using the `passphrase`.
    pub fn create_recovery_file(keypair: &Keypair, passphrase: &str) -> Result<Vec<u8>> {
        Ok(create_recovery_file(keypair, passphrase)?)
    }

    /// Recover a keypair from a recovery file by decrypting the secret key using `passphrase`.
    pub fn decrypt_recovery_file(recovery_file: &[u8], passphrase: &str) -> Result<Keypair> {
        Ok(decrypt_recovery_file(recovery_file, passphrase)?)
    }
}
