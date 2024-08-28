//! Client-server Authentication using signed timesteps

use std::sync::{Arc, Mutex};

use ed25519_dalek::ed25519::SignatureBytes;
use serde::{Deserialize, Serialize};

use crate::{
    crypto::{random_hash, Keypair, PublicKey, Signature},
    timestamp::Timestamp,
};

// 30 seconds
const TIME_INTERVAL: u64 = 30 * 1_000_000;

const CURRENT_VERSION: u8 = 0;
// 45 seconds in the past or the future
const TIMESTAMP_WINDOW: i64 = 45 * 1_000_000;

#[derive(Debug, PartialEq)]
pub struct AuthnSignature(Box<[u8]>);

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthToken {
    /// Version of the [AuthToken].
    ///
    /// Version 0: Signer is implicitly the same as the [AuthToken::subject]
    version: u8,
    /// Signature over the token.
    signature: Signature,
    /// Timestamp
    timestamp: Timestamp,
    /// The [PublicKey] of the owner of the resources being accessed by this token.
    subject: PublicKey,
    /// The Pubky of the party verifying the [AuthToken], for example a web server.
    audience: PublicKey,
    // Variable length scopes
    scopes: Vec<String>,
}

impl AuthToken {
    pub fn sign(signer: &Keypair, audience: &PublicKey, scopes: Vec<String>) -> Self {
        let timestamp = Timestamp::now();

        let mut token = Self {
            version: 0,
            subject: signer.public_key(),
            audience: audience.to_owned(),
            timestamp,
            scopes,
            signature: Signature::from_bytes(&[0; 64]),
        };

        let serialized = token.serialize();

        token.signature = signer.sign(&serialized[65..]);

        token
    }

    fn verify(audience: &PublicKey, bytes: &[u8]) -> Result<Self, Error> {
        if bytes[0] > CURRENT_VERSION {
            return Err(Error::UnknownVersion);
        }

        let token: AuthToken = postcard::from_bytes(bytes)?;

        match token.version {
            0 => {
                let now = Timestamp::now();

                if &token.audience != audience {
                    return Err(Error::InvalidAudience(
                        audience.to_string(),
                        token.audience.to_string(),
                    ));
                }

                // Chcek timestamp;
                let diff = token.timestamp.difference(&now);
                if diff > TIMESTAMP_WINDOW {
                    return Err(Error::TooFarInTheFuture);
                }
                if diff < -TIMESTAMP_WINDOW {
                    return Err(Error::Expired);
                }

                token
                    .subject
                    .verify(AuthToken::signable(token.version, bytes), &token.signature)
                    .map_err(|_| Error::InvalidSignature)?;

                Ok(token)
            }
            _ => unreachable!(),
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    /// A unique ID for this [AuthToken], which is a concatenation of
    /// [AuthToken::subject] and [AuthToken::timestamp].
    ///
    /// Assuming that [AuthToken::timestamp] is unique for every [AuthToken::subject].
    fn id(version: u8, bytes: &[u8]) -> Box<[u8]> {
        match version {
            0 => bytes[65..105].into(),
            _ => unreachable!(),
        }
    }

    fn signable(version: u8, bytes: &[u8]) -> &[u8] {
        match version {
            0 => bytes[65..].into(),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthVerifier {
    audience: PublicKey,
    seen: Arc<Mutex<Vec<Box<[u8]>>>>,
}

impl AuthVerifier {
    pub fn new(audience: PublicKey) -> Self {
        Self {
            audience,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<AuthToken, Error> {
        self.gc();

        let token = AuthToken::verify(&self.audience, bytes)?;

        let mut seen = self.seen.lock().unwrap();

        let id = AuthToken::id(token.version, bytes);

        match seen.binary_search_by(|element| element.cmp(&id)) {
            Ok(_) => Err(Error::AlreadyUsed),
            Err(index) => {
                seen.insert(index, id.into());
                Ok(token)
            }
        }
    }

    // === Private Methods ===

    /// Remove all tokens older than two time intervals in the past.
    fn gc(&self) {
        let threshold = ((Timestamp::now().into_inner() / TIME_INTERVAL) - 2).to_be_bytes();

        let mut inner = self.seen.lock().unwrap();

        match inner.binary_search_by(|element| element[0..8].cmp(&threshold)) {
            Ok(index) | Err(index) => {
                inner.drain(0..index);
            }
        }
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum Error {
    #[error("Unknown version")]
    UnknownVersion,
    #[error("Invalid audience. Expected {0}, got {1}")]
    InvalidAudience(String, String),
    #[error("AuthToken has a timestamp that is more than 45 seconds in the future")]
    TooFarInTheFuture,
    #[error("AuthToken has a timestamp that is more than 45 seconds in the past")]
    Expired,
    #[error("Invalid Signature")]
    InvalidSignature,
    #[error(transparent)]
    Postcard(#[from] postcard::Error),
    #[error("AuthToken already used")]
    AlreadyUsed,
}

impl AuthnSignature {
    pub fn new(signer: &Keypair, audience: &PublicKey, token: Option<&[u8]>) -> Self {
        let mut bytes = Vec::with_capacity(96);

        let time: u64 = Timestamp::now().into();
        let time_step = time / TIME_INTERVAL;

        let token_hash = token.map_or(random_hash(), crate::crypto::hash);

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

    /// Sign a randomly generated nonce
    pub fn generate(keypair: &Keypair, audience: &PublicKey) -> Self {
        AuthnSignature::new(keypair, audience, None)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Debug, Clone)]
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

        let result = verify_at(now, self, &signature, signer, &token_hash);

        match result {
            Ok(_) => return Ok(()),
            Err(AuthnSignatureError::AlreadyUsed) => return Err(AuthnSignatureError::AlreadyUsed),
            _ => {}
        }

        let result = verify_at(past, self, &signature, signer, &token_hash);

        match result {
            Ok(_) => return Ok(()),
            Err(AuthnSignatureError::AlreadyUsed) => return Err(AuthnSignatureError::AlreadyUsed),
            _ => {}
        }

        verify_at(future, self, &signature, signer, &token_hash)
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

        let mut candidate = [0_u8; 40];
        candidate[..8].copy_from_slice(&time_step_bytes);
        candidate[8..].copy_from_slice(token_hash);

        match inner.binary_search_by(|element| element.cmp(&candidate)) {
            Ok(index) | Err(index) => {
                inner.insert(index, candidate);
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
) -> [u8; 115] {
    let mut arr = [0; 115];

    arr[..11].copy_from_slice(crate::namespaces::PUBKY_AUTHN);
    arr[19..51].copy_from_slice(signer.as_bytes());
    arr[11..19].copy_from_slice(time_step_bytes);
    arr[51..83].copy_from_slice(audience.as_bytes());
    arr[83..].copy_from_slice(token_hash);

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
    use crate::{auth::TIMESTAMP_WINDOW, crypto::Keypair, timestamp::Timestamp};

    use super::{AuthToken, AuthVerifier, Error};

    #[test]
    fn v0_id_signable() {
        let signer = Keypair::random();
        let audience = Keypair::random().public_key();
        let scopes = vec!["*:*".to_string()];

        let token = AuthToken::sign(&signer, &audience, scopes.clone());

        let serialized = &token.serialize();

        assert_eq!(
            AuthToken::id(token.version, serialized),
            serialized[65..105].into()
        );

        assert_eq!(
            AuthToken::signable(token.version, serialized),
            &serialized[65..]
        )
    }

    #[test]
    fn sign_verify() {
        let signer = Keypair::random();
        let audience = Keypair::random().public_key();
        let scopes = vec!["*:*".to_string()];

        let verifier = AuthVerifier::new(audience.clone());

        let token = AuthToken::sign(&signer, &audience, scopes.clone());

        let serialized = &token.serialize();

        verifier.verify(serialized).unwrap();

        assert_eq!(token.scopes, scopes);
    }

    #[test]
    fn expired() {
        let signer = Keypair::random();
        let audience = Keypair::random().public_key();
        let scopes = vec!["*:*".to_string()];

        let verifier = AuthVerifier::new(audience.clone());

        let timestamp = (&Timestamp::now()) - (TIMESTAMP_WINDOW as u64);

        let mut signable = vec![];
        signable.extend_from_slice(signer.public_key().as_bytes());
        signable.extend_from_slice(audience.as_bytes());
        signable.extend_from_slice(&postcard::to_allocvec(&scopes).unwrap());

        let signature = signer.sign(&signable);

        let token = AuthToken {
            version: 0,
            subject: signer.public_key(),
            audience,
            timestamp,
            signature,
            scopes,
        };

        let serialized = token.serialize();

        let result = verifier.verify(&serialized);

        assert_eq!(result, Err(Error::Expired));
    }
}
