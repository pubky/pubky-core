use rand::random;

use crate::{
    crypto::{blake3, PublicKey, Signature},
    error::{Error, Result},
    time::Timestamp,
};

pub const CONTEXT: &str = "pubky:homeserver:auth:challenge";

/// Default time to live for [AuthChallenge]: one minute
const DEFAULT_TTL: u64 = 60 * 1000 * 1000;

/// Homeserver generated challenge to be signed by a client's private key
/// for authentication (signup / login).
///
/// Encoded as `<32 bytes nonce><8 bytes Big-Endian [crate::time::Timestamp]>`
#[derive(Debug, PartialEq)]
pub struct AuthChallenge([u8; 40]);

impl Default for AuthChallenge {
    fn default() -> Self {
        Self::new(DEFAULT_TTL)
    }
}

impl AuthChallenge {
    pub fn new(ttl: u64) -> Self {
        let mut bytes = [0u8; 40];

        let key: [u8; blake3::KEY_LEN] = random();
        bytes[0..32].copy_from_slice(&key);

        let expires_at = Timestamp::now() + ttl;
        expires_at.encode(&mut bytes[32..]);

        Self(bytes)
    }

    /// Returns the nonce part of this challenge
    pub fn nonce(&self) -> &[u8] {
        &self.0[0..32]
    }

    /// Returns the [Timestamp] at which this challenge should expires
    pub fn expires_at(&self) -> Timestamp {
        let bytes: [u8; 8] = self.0[32..].try_into().unwrap();

        Timestamp(u64::from_be_bytes(bytes))
    }

    /// Returns the full encoded challenge
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Returns whether or not this challenge is expired
    pub fn expired(&self) -> bool {
        self.expires_at() <= Timestamp::now()
    }

    /// Return the signable of which is the result of hashing
    /// the concatenation of the [CONTEXT] and the [Self::nonce] of this challenge.
    pub fn signable(&self) -> [u8; blake3::KEY_LEN] {
        blake3::derive_key(CONTEXT, self.nonce())
    }

    /// Verify a signature over this challenge's [Self::signable]
    pub fn verify(
        self: &Self,
        public_key: &PublicKey,
        signature: impl Into<Signature>,
    ) -> Result<()> {
        // TODO: document errors

        if self.expired() {
            return Err(Error::Generic("Expired challenge".to_string()));
        }

        public_key
            .verify(&self.signable(), &signature.into())
            .map_err(|_| Error::Generic("Invalid signature".to_string()))
    }
}

impl TryFrom<&[u8]> for AuthChallenge {
    type Error = Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        let bytes: [u8; 40] = bytes.try_into().map_err(|_| {
            Error::Generic(format!(
                "Invalid AuthChallenge bytes size, expected 40, got: {}",
                bytes.len()
            ))
        })?;

        Ok(Self(bytes))
    }
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::AuthChallenge;

    #[test]
    fn expiry() {
        let challenge = AuthChallenge::new(0);

        assert!(challenge.expired())
    }

    #[test]
    fn serialization() {
        let challenge = AuthChallenge::default();

        let decoded = AuthChallenge::try_from(challenge.as_bytes()).unwrap();

        assert_eq!(decoded, challenge)
    }

    #[test]
    fn sign_verify() {
        let challenge = AuthChallenge::default();

        let keypair = Keypair::random();

        challenge
            .verify(&keypair.public_key(), keypair.sign(&challenge.signable()))
            .unwrap();

        {
            // Invalid signable
            assert!(!challenge
                .verify(&keypair.public_key(), keypair.sign(&challenge.as_bytes()))
                .is_ok())
        }

        {
            // Invalid signer
            assert!(!challenge
                .verify(
                    &Keypair::random().public_key(),
                    keypair.sign(&challenge.signable())
                )
                .is_ok())
        }

        {
            // Expired challenge
            let challenge = AuthChallenge::new(0);

            // Invalid signable
            assert!(!challenge
                .verify(&keypair.public_key(), keypair.sign(&challenge.signable()))
                .is_ok())
        }
    }
}
