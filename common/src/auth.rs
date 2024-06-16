use ed25519_dalek::ed25519::SignatureBytes;

use crate::{
    crypto::{Hash, Keypair, PublicKey, Signature},
    time::Timestamp,
};

// Tolerate 45 seconds in the past (delays) or in the future (clock drift)
pub const MAX_AUTHN_SIGNATURE_DIFF: u64 = 90_000_000;

pub const EMPTY_HASH: Hash = Hash::from_bytes([
    175, 19, 73, 185, 245, 249, 161, 166, 160, 64, 77, 234, 54, 220, 201, 73, 155, 203, 37, 201,
    173, 193, 18, 183, 204, 154, 147, 202, 228, 31, 50, 98,
]);

// 30 seconds
const TIME_INTERVAL: u64 = 30 * 1000_000;

#[derive(Debug)]
pub struct AuthnVerified {
    pub public_key: PublicKey,
    pub hash: Hash,
}

#[derive(Debug, PartialEq)]
pub struct AuthnSignature(Box<[u8]>);

impl AuthnSignature {
    pub fn new(signer: &Keypair, audience: &PublicKey, token: Option<&[u8]>) -> Self {
        let mut bytes = Vec::with_capacity(96);

        let time: u64 = Timestamp::now().into();
        let time_step = time / TIME_INTERVAL;

        let token_hash = token.map_or(EMPTY_HASH, |t| crate::crypto::hash(t));

        let signature = signer
            .sign(&signable(
                &signer.public_key(),
                audience,
                time_step,
                token_hash,
            ))
            .to_bytes();

        bytes.extend_from_slice(&signature);
        bytes.extend_from_slice(token_hash.as_bytes());

        Self(bytes.into())
    }

    pub fn for_token(keypair: &Keypair, audience: &PublicKey, token: &[u8]) -> Self {
        AuthnSignature::new(keypair, audience, Some(token))
    }

    // === Getters ===

    /// Return the `<timestamp><issuer's public_key>` as a unique time sortable identifier
    pub fn id(&self) -> [u8; 40] {
        self.0[72..112].try_into().unwrap()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    // === Public Methods ===

    pub fn verify(
        bytes: &[u8],
        signer: &PublicKey,
        audience: &PublicKey,
    ) -> Result<AuthnVerified, AuthnSignatureError> {
        if bytes.len() != 96 {
            return Err(AuthnSignatureError::InvalidLength(bytes.len()));
        }

        // TODO: better invalid length error
        let signature_bytes: SignatureBytes = bytes[0..64]
            .try_into()
            .expect("validate token length on instantiating");
        let signature = Signature::from(signature_bytes);

        let now = Timestamp::now();

        let time_step = now.into_inner() / TIME_INTERVAL;

        let hash_bytes: [u8; 32] = bytes[64..].try_into().expect("should not be reachable");

        let hash: Hash = hash_bytes.into();

        signer
            .verify(&signable(signer, audience, time_step, hash), &signature)
            // TODO: try earlier and later time_step if the current failed.
            .map_err(|_| AuthnSignatureError::InvalidSignature)?;

        Ok(AuthnVerified {
            public_key: signer.to_owned(),
            hash,
        })
    }
}

fn signable(signer: &PublicKey, audience: &PublicKey, time_step: u64, token_hash: Hash) -> Vec<u8> {
    let mut vec = Vec::with_capacity(64 + 8 + 32);

    vec.extend_from_slice(crate::namespaces::PK_AUTHN);
    vec.extend_from_slice(&time_step.to_be_bytes());
    vec.extend_from_slice(signer.as_bytes());
    vec.extend_from_slice(audience.as_bytes());
    vec.extend_from_slice(token_hash.as_bytes());

    vec
}

#[derive(thiserror::Error, Debug)]
pub enum AuthnSignatureError {
    #[error("AuthnSignature should be 96 bytes long, got {0} bytes instead")]
    InvalidLength(usize),

    #[error("AuthnSignature is too old")]
    TooOld,

    #[error("AuthnSignature is too far in the future")]
    TooFarInTheFuture,

    #[error("AuthnSignature is meant for a different audience")]
    InvalidAudience,

    #[error("Invalid Timestamp")]
    InvalidTimestamp(#[from] crate::time::TimestampError),

    #[error("Invalid signer public_key")]
    InvalidSigner,

    #[error("Invalid signature")]
    InvalidSignature,
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::AuthnSignature;

    #[test]
    fn sign_verify() {
        let keypair = Keypair::random();
        let signer = keypair.public_key();
        let audience = Keypair::random().public_key();

        let authn_signature = AuthnSignature::new(&keypair, &audience, None);

        AuthnSignature::verify(authn_signature.as_bytes(), &signer, &audience).unwrap();

        {
            // Invalid signable
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[64..].copy_from_slice(&[0; 32]);

            assert!(!AuthnSignature::verify(&invalid, &signer, &audience).is_ok())
        }

        {
            // Invalid signer
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[0..32].copy_from_slice(&[0; 32]);

            assert!(!AuthnSignature::verify(&invalid, &signer, &audience).is_ok())
        }
    }
}
