use std::sync::{Arc, Mutex};

use ed25519_dalek::ed25519::SignatureBytes;

use crate::{
    crypto::{random_hash, Keypair, PublicKey, Signature},
    time::Timestamp,
};

// 30 seconds
const TIME_INTERVAL: u64 = 30 * 1_000_000;

#[derive(Debug, PartialEq)]
pub struct AuthnSignature(Box<[u8]>);

impl AuthnSignature {
    pub fn new(signer: &Keypair, audience: &PublicKey, token: Option<&[u8]>) -> Self {
        let mut bytes = Vec::with_capacity(96);

        let time: u64 = Timestamp::now().into();
        let time_step = time / TIME_INTERVAL;

        let token_hash = token.map_or(random_hash(), |t| crate::crypto::hash(t));

        let signature = signer
            .sign(&signable(
                &time_step.to_be_bytes(),
                &signer.public_key(),
                audience,
                token_hash.as_bytes(),
            ))
            .to_bytes();

        bytes.extend_from_slice(&signature);
        bytes.extend_from_slice(token_hash.as_bytes());

        Self(bytes.into())
    }

    pub fn for_token(keypair: &Keypair, audience: &PublicKey, token: &[u8]) -> Self {
        AuthnSignature::new(keypair, audience, Some(token))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub struct AuthnVerifier {
    audience: PublicKey,
    inner: Arc<Mutex<Vec<[u8; 40]>>>,
    // TODO: Support permisisons
    // token_hashes: HashSet<[u8; 32]>,
}

impl AuthnVerifier {
    pub fn new(audience: PublicKey) -> Self {
        Self {
            audience,
            inner: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn verify(&self, bytes: &[u8], signer: &PublicKey) -> Result<(), AuthnSignatureError> {
        self.gc();

        if bytes.len() != 96 {
            return Err(AuthnSignatureError::InvalidLength(bytes.len()));
        }

        let signature_bytes: SignatureBytes = bytes[0..64]
            .try_into()
            .expect("validate token length on instantiating");
        let signature = Signature::from(signature_bytes);

        let token_hash: [u8; 32] = bytes[64..].try_into().expect("should not be reachable");

        let now = Timestamp::now().into_inner();
        let past = now - TIME_INTERVAL;
        let future = now + TIME_INTERVAL;

        let result = verify_at(now, &self, &signature, signer, &token_hash);

        match result {
            Ok(_) => return Ok(()),
            Err(AuthnSignatureError::AlreadyUsed) => return Err(AuthnSignatureError::AlreadyUsed),
            _ => {}
        }

        let result = verify_at(past, &self, &signature, signer, &token_hash);

        match result {
            Ok(_) => return Ok(()),
            Err(AuthnSignatureError::AlreadyUsed) => return Err(AuthnSignatureError::AlreadyUsed),
            _ => {}
        }

        verify_at(future, &self, &signature, signer, &token_hash)
    }

    // === Private Methods ===

    /// Remove all tokens older than two time intervals in the past.
    fn gc(&self) {
        let threshold = ((Timestamp::now().into_inner() / TIME_INTERVAL) - 2).to_be_bytes();

        let mut inner = self.inner.lock().unwrap();

        match inner.binary_search_by(|element| element[0..8].cmp(&threshold)) {
            Ok(index) | Err(index) => {
                inner.drain(0..index);
            }
        }
    }
}

fn verify_at(
    time: u64,
    verifier: &AuthnVerifier,
    signature: &Signature,
    signer: &PublicKey,
    token_hash: &[u8; 32],
) -> Result<(), AuthnSignatureError> {
    let time_step = time / TIME_INTERVAL;
    let time_step_bytes = time_step.to_be_bytes();

    let result = signer.verify(
        &signable(&time_step_bytes, signer, &verifier.audience, token_hash),
        signature,
    );

    if result.is_ok() {
        let mut inner = verifier.inner.lock().unwrap();

        match inner.binary_search_by(|element| element[0..8].cmp(&time_step_bytes)) {
            Ok(index) | Err(index) => {
                let mut array40: [u8; 40] = [0; 40];

                array40[..8].copy_from_slice(&time_step_bytes);
                array40[8..].copy_from_slice(token_hash);

                inner.insert(index, array40);
            }
        };

        return Ok(());
    }

    Err(AuthnSignatureError::InvalidSignature)
}

fn signable(
    time_step_bytes: &[u8; 8],
    signer: &PublicKey,
    audience: &PublicKey,
    token_hash: &[u8; 32],
) -> [u8; 112] {
    let mut arr = [0; 112];

    arr[..8].copy_from_slice(crate::namespaces::PK_AUTHN);
    arr[8..16].copy_from_slice(time_step_bytes);
    arr[16..48].copy_from_slice(signer.as_bytes());
    arr[48..80].copy_from_slice(audience.as_bytes());
    arr[80..].copy_from_slice(token_hash);

    arr
}

#[derive(thiserror::Error, Debug)]
pub enum AuthnSignatureError {
    #[error("AuthnSignature should be 96 bytes long, got {0} bytes instead")]
    InvalidLength(usize),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Authn signature already used")]
    AlreadyUsed,
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::{AuthnSignature, AuthnVerifier};

    #[test]
    fn sign_verify() {
        let keypair = Keypair::random();
        let signer = keypair.public_key();
        let audience = Keypair::random().public_key();

        let verifier = AuthnVerifier::new(audience.clone());

        let authn_signature = AuthnSignature::new(&keypair, &audience, None);

        verifier
            .verify(authn_signature.as_bytes(), &signer)
            .unwrap();

        {
            // Invalid signable
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[64..].copy_from_slice(&[0; 32]);

            assert!(!verifier.verify(&invalid, &signer).is_ok())
        }

        {
            // Invalid signer
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[0..32].copy_from_slice(&[0; 32]);

            assert!(!verifier.verify(&invalid, &signer).is_ok())
        }
    }
}
