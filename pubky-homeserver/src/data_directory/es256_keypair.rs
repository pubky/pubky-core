use anyhow::{anyhow, Result};
use generic_array::GenericArray;
use p256::{
    ecdsa::{SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey},
    SecretKey,
};
use rand_core::OsRng;

/// ES256 key pair (ECDSA with P-256 curve).
/// Used for signing and verifying JWT tokens.
/// ES256 is the most popular algorithm for JWT.
/// Would be better to use Ed25519 which is the pkarr curve, but it's not supported by JWT.
#[derive(Clone, Debug)]
pub struct ES256KeyPair {
    pub private_key: SigningKey,
}

impl ES256KeyPair {
    /// Generate a new random ES256 key pair
    pub fn random() -> Result<Self> {
        let private_key = SigningKey::random(&mut OsRng);

        Ok(Self { private_key })
    }

    /// Create a new ES256 key pair from a secret key
    fn from_secret_key(secret_key: &[u8; 32]) -> Result<Self> {
        let array = GenericArray::from_slice(secret_key);
        let secret =
            SecretKey::from_bytes(array).map_err(|e| anyhow!("Invalid private key: {}", e))?;
        let private_key = SigningKey::from(secret);

        Ok(Self { private_key })
    }

    /// Derive a new ES256 key pair from the main homeserver secret
    pub fn derive_from_main_secret_key(main_secret_key: &[u8; 32]) -> Result<Self> {
        let hkdf = hkdf::Hkdf::<sha2::Sha256>::from_prk(main_secret_key)
            .expect("main secret key is large enough to be used as a PRK");
        let mut okm = [0u8; 32];
        hkdf.expand(b"jwt-signing-key", &mut okm)
            .expect("32 is a valid output length");
        Self::from_secret_key(&okm)
    }

    /// Get the public key
    pub fn public_key(&self) -> &VerifyingKey {
        self.private_key.verifying_key()
    }

    /// Get the private key as a PEM string
    pub fn private_key_pem(&self) -> Result<String, p256::pkcs8::Error> {
        Ok(self
            .private_key
            .to_pkcs8_pem(Default::default())?
            .to_string())
    }

    /// Get the public key as a PEM string
    pub fn public_key_pem(&self) -> Result<String, p256::pkcs8::Error> {
        Ok(self
            .public_key()
            .to_public_key_pem(Default::default())?
            .to_string())
    }

    /// Load an ES256 key from a PEM file
    pub fn from_pem(private_key_pem: &str) -> Result<Self> {
        // Validate the keys by trying to parse them
        let private_key = SigningKey::from_pkcs8_pem(private_key_pem)
            .map_err(|e| anyhow!("Invalid private key: {}", e))?;

        Ok(Self { private_key })
    }
}

#[cfg(test)]
mod tests {
    use jsonwebtoken::{Algorithm, EncodingKey, Header};

    use super::*;

    #[test]
    fn test_save_and_read() -> Result<()> {
        let key_pair = ES256KeyPair::random()?;

        // Check that we can save and load keys
        let private_key_pem = key_pair.private_key_pem()?;

        // Check that we can read the keys back
        let loaded_key_pair = ES256KeyPair::from_pem(&private_key_pem)?;

        assert_eq!(key_pair.private_key, loaded_key_pair.private_key);
        assert_eq!(key_pair.public_key(), loaded_key_pair.public_key());
        Ok(())
    }

    #[test]
    fn test_derive_from_main_secret_key() {
        let main_secret_key = [0u8; 32];
        let key_pair = ES256KeyPair::derive_from_main_secret_key(&main_secret_key).unwrap();
        let public_key_pem = key_pair.private_key_pem().unwrap();

        let should = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgo6sJpORWZC1Z/gGM\nwRNHEDGskAgU3Tf1c52lDi5QkYehRANCAATu8ZS9A3Eer1B1tFjTyGwQxh2sDBVG\nx3V+ycvAw97UZ1PpiU1J6cRsuiugmPcgLzKIDU46U5wFzATLHDgNT/+C\n-----END PRIVATE KEY-----\n";
        assert_eq!(public_key_pem, should);
    }

    #[test]
    fn test_pem_to_jwt_key() {
        let key_pair = ES256KeyPair::random().unwrap();
        let parsed_pem = key_pair.private_key_pem().unwrap();

        EncodingKey::from_ec_pem(parsed_pem.as_bytes()).unwrap();
    }
}
