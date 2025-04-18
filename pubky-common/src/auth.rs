//! Client-server Authentication using signed timesteps

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    capabilities::{Capabilities, Capability},
    crypto::{Keypair, PublicKey, Signature},
    namespaces::PUBKY_AUTH,
    timestamp::Timestamp,
};

// 30 seconds
const TIME_INTERVAL: u64 = 30 * 1_000_000;

const CURRENT_VERSION: u8 = 0;
// 45 seconds in the past or the future
const TIMESTAMP_WINDOW: i64 = 45 * 1_000_000;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
/// Implementation of the [Pubky Auth spec](https://pubky.github.io/pubky-core/spec/auth.html).
pub struct AuthToken {
    /// Signature over the token.
    signature: Signature,
    /// A namespace to ensure this signature can't be used for any
    /// other purposes that share the same message structurea by accident.
    namespace: [u8; 10],
    /// Version of the [AuthToken], in case we need to upgrade it to support unforeseen usecases.
    ///
    /// Version 0:
    /// - Signer is implicitly the same as the root keypair for
    ///   the [AuthToken::pubky], without any delegation.
    /// - Capabilities are only meant for resoucres on the homeserver.
    version: u8,
    /// Timestamp
    timestamp: Timestamp,
    /// The [PublicKey] of the owner of the resources being accessed by this token.
    pubky: PublicKey,
    // Variable length capabilities
    capabilities: Capabilities,
}

impl AuthToken {
    /// Sign a new AuthToken with given capabilities.
    pub fn sign(keypair: &Keypair, capabilities: impl Into<Capabilities>) -> Self {
        let timestamp = Timestamp::now();

        let mut token = Self {
            signature: Signature::from_bytes(&[0; 64]),
            namespace: *PUBKY_AUTH,
            version: 0,
            timestamp,
            pubky: keypair.public_key(),
            capabilities: capabilities.into(),
        };

        let serialized = token.serialize();

        token.signature = keypair.sign(&serialized[65..]);

        token
    }

    // === Getters ===

    /// Returns the pubky that is providing this AuthToken
    pub fn pubky(&self) -> &PublicKey {
        &self.pubky
    }

    /// Returns the capabilities in this AuthToken.
    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities.0
    }

    // === Public Methods ===

    /// Parse and verify an AuthToken.
    pub fn verify(bytes: &[u8]) -> Result<Self, Error> {
        if bytes[75] > CURRENT_VERSION {
            return Err(Error::UnknownVersion);
        }

        let token = AuthToken::deserialize(bytes)?;

        match token.version {
            0 => {
                let now = Timestamp::now();

                // Chcek timestamp;
                let diff = token.timestamp.as_u64() as i64 - now.as_u64() as i64;
                if diff > TIMESTAMP_WINDOW {
                    return Err(Error::TooFarInTheFuture);
                }
                if diff < -TIMESTAMP_WINDOW {
                    return Err(Error::Expired);
                }

                token
                    .pubky
                    .verify(AuthToken::signable(token.version, bytes), &token.signature)
                    .map_err(|_| Error::InvalidSignature)?;

                Ok(token)
            }
            _ => unreachable!(),
        }
    }

    /// Serialize this AuthToken to its canonical binary representation.
    pub fn serialize(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap()
    }

    /// Deserialize an AuthToken from its canonical binary representation.
    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        Ok(postcard::from_bytes(bytes)?)
    }

    /// Returns the unique ID for this [AuthToken], which is a concatenation of
    /// [AuthToken::pubky] and [AuthToken::timestamp].
    ///
    /// Assuming that [AuthToken::timestamp] is unique for every [AuthToken::pubky].
    fn id(version: u8, bytes: &[u8]) -> Box<[u8]> {
        match version {
            0 => bytes[75..115].into(),
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

#[derive(Debug, Clone, Default)]
/// Keeps track of used AuthToken until they expire.
pub struct AuthVerifier {
    seen: Arc<Mutex<Vec<Box<[u8]>>>>,
}

impl AuthVerifier {
    /// Verify an [AuthToken] by parsing it from its canonical binary representation,
    /// verifying its signature, and confirm it wasn't already used.
    pub fn verify(&self, bytes: &[u8]) -> Result<AuthToken, Error> {
        self.gc();

        let token = AuthToken::verify(bytes)?;

        let mut seen = self.seen.lock().unwrap();

        let id = AuthToken::id(token.version, bytes);

        match seen.binary_search_by(|element| element.cmp(&id)) {
            Ok(_) => Err(Error::AlreadyUsed),
            Err(index) => {
                seen.insert(index, id);
                Ok(token)
            }
        }
    }

    // === Private Methods ===

    /// Remove all tokens older than two time intervals in the past.
    fn gc(&self) {
        let threshold = ((Timestamp::now().as_u64() / TIME_INTERVAL) - 2).to_be_bytes();

        let mut inner = self.seen.lock().unwrap();

        match inner.binary_search_by(|element| element[0..8].cmp(&threshold)) {
            Ok(index) | Err(index) => {
                inner.drain(0..index);
            }
        }
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
/// Error verifying an [AuthToken]
pub enum Error {
    #[error("Unknown version")]
    /// Unknown version
    UnknownVersion,
    #[error("AuthToken has a timestamp that is more than 45 seconds in the future")]
    /// AuthToken has a timestamp that is more than 45 seconds in the future
    TooFarInTheFuture,
    #[error("AuthToken has a timestamp that is more than 45 seconds in the past")]
    /// AuthToken has a timestamp that is more than 45 seconds in the past
    Expired,
    #[error("Invalid Signature")]
    /// Invalid Signature
    InvalidSignature,
    #[error(transparent)]
    /// Error parsing [AuthToken] using Postcard
    Parsing(#[from] postcard::Error),
    #[error("AuthToken already used")]
    /// AuthToken already used
    AlreadyUsed,
}

#[cfg(test)]
mod tests {
    use crate::{
        auth::TIMESTAMP_WINDOW, capabilities::Capability, crypto::Keypair, timestamp::Timestamp,
    };

    use super::*;

    #[test]
    fn v0_id_signable() {
        let signer = Keypair::random();
        let capabilities = vec![Capability::root()];

        let token = AuthToken::sign(&signer, capabilities.clone());

        let serialized = &token.serialize();

        let mut id = vec![];
        id.extend_from_slice(&token.timestamp.to_bytes());
        id.extend_from_slice(signer.public_key().as_bytes());

        assert_eq!(AuthToken::id(token.version, serialized), id.into());

        assert_eq!(
            AuthToken::signable(token.version, serialized),
            &serialized[65..]
        )
    }

    #[test]
    fn sign_verify() {
        let signer = Keypair::random();
        let capabilities = vec![Capability::root()];

        let verifier = AuthVerifier::default();

        let token = AuthToken::sign(&signer, capabilities.clone());

        let serialized = &token.serialize();

        verifier.verify(serialized).unwrap();

        assert_eq!(token.capabilities, capabilities.into());
    }

    #[test]
    fn expired() {
        let signer = Keypair::random();
        let capabilities = Capabilities(vec![Capability::root()]);

        let verifier = AuthVerifier::default();

        let timestamp = (Timestamp::now()) - (TIMESTAMP_WINDOW as u64);

        let mut signable = vec![];
        signable.extend_from_slice(signer.public_key().as_bytes());
        signable.extend_from_slice(&postcard::to_allocvec(&capabilities).unwrap());

        let signature = signer.sign(&signable);

        let token = AuthToken {
            signature,
            namespace: *PUBKY_AUTH,
            version: 0,
            timestamp,
            pubky: signer.public_key(),
            capabilities,
        };

        let serialized = token.serialize();

        let result = verifier.verify(&serialized);

        assert_eq!(result, Err(Error::Expired));
    }

    #[test]
    fn already_used() {
        let signer = Keypair::random();
        let capabilities = vec![Capability::root()];

        let verifier = AuthVerifier::default();

        let token = AuthToken::sign(&signer, capabilities.clone());

        let serialized = &token.serialize();

        verifier.verify(serialized).unwrap();

        assert_eq!(token.capabilities, capabilities.into());

        assert_eq!(verifier.verify(serialized), Err(Error::AlreadyUsed));
    }
}
