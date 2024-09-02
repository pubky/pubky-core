//! Client-server Authentication using signed timesteps

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    capabilities::Capability,
    crypto::{Keypair, PublicKey, Signature},
    timestamp::Timestamp,
};

// 30 seconds
const TIME_INTERVAL: u64 = 30 * 1_000_000;

const CURRENT_VERSION: u8 = 0;
// 45 seconds in the past or the future
const TIMESTAMP_WINDOW: i64 = 45 * 1_000_000;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct AuthToken {
    /// Version of the [AuthToken].
    ///
    /// Version 0: Signer is implicitly the same as the root keypair for
    ///     the [AuthToken::pubky], without any delegation.
    version: u8,
    /// Signature over the token.
    signature: Signature,
    /// Timestamp
    timestamp: Timestamp,
    /// The [PublicKey] of the owner of the resources being accessed by this token.
    pubky: PublicKey,
    /// The Pubky of the party verifying the [AuthToken], for example a web server.
    homeserver: PublicKey,
    // Variable length capabilities
    capabilities: Vec<Capability>,
}

impl AuthToken {
    pub fn sign(keypair: &Keypair, homeserver: &PublicKey, capabilities: Vec<Capability>) -> Self {
        let timestamp = Timestamp::now();

        let mut token = Self {
            version: 0,
            pubky: keypair.public_key(),
            homeserver: homeserver.to_owned(),
            timestamp,
            capabilities,
            signature: Signature::from_bytes(&[0; 64]),
        };

        let serialized = token.serialize();

        token.signature = keypair.sign(&serialized[65..]);

        token
    }

    pub fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    fn verify(homeserver: &PublicKey, bytes: &[u8]) -> Result<Self, Error> {
        if bytes[0] > CURRENT_VERSION {
            return Err(Error::UnknownVersion);
        }

        let token = AuthToken::deserialize(bytes)?;

        match token.version {
            0 => {
                let now = Timestamp::now();

                if &token.homeserver != homeserver {
                    return Err(Error::InvalidHomeserver(
                        homeserver.to_string(),
                        token.homeserver.to_string(),
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
                    .pubky
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

    pub fn deserialize(bytes: &[u8]) -> Result<Self, Error> {
        Ok(postcard::from_bytes(bytes)?)
    }

    pub fn pubky(&self) -> &PublicKey {
        &self.pubky
    }

    /// A unique ID for this [AuthToken], which is a concatenation of
    /// [AuthToken::pubky] and [AuthToken::timestamp].
    ///
    /// Assuming that [AuthToken::timestamp] is unique for every [AuthToken::pubky].
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
    homeserver: PublicKey,
    seen: Arc<Mutex<Vec<Box<[u8]>>>>,
}

impl AuthVerifier {
    pub fn new(homeserver: PublicKey) -> Self {
        Self {
            homeserver,
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<AuthToken, Error> {
        self.gc();

        let token = AuthToken::verify(&self.homeserver, bytes)?;

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
    #[error("Invalid homeserver. Expected {0}, got {1}")]
    InvalidHomeserver(String, String),
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

#[cfg(test)]
mod tests {
    use crate::{
        auth::TIMESTAMP_WINDOW, capabilities::Capability, crypto::Keypair, timestamp::Timestamp,
    };

    use super::{AuthToken, AuthVerifier, Error};

    #[test]
    fn v0_id_signable() {
        let signer = Keypair::random();
        let homeserver = Keypair::random().public_key();
        let capabilities = vec![Capability::root()];

        let token = AuthToken::sign(&signer, &homeserver, capabilities.clone());

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
        let homeserver = Keypair::random().public_key();
        let capabilities = vec![Capability::root()];

        let verifier = AuthVerifier::new(homeserver.clone());

        let token = AuthToken::sign(&signer, &homeserver, capabilities.clone());

        let serialized = &token.serialize();

        verifier.verify(serialized).unwrap();

        assert_eq!(token.capabilities, capabilities);
    }

    #[test]
    fn expired() {
        let signer = Keypair::random();
        let homeserver = Keypair::random().public_key();
        let capabilities = vec![Capability::root()];

        let verifier = AuthVerifier::new(homeserver.clone());

        let timestamp = (&Timestamp::now()) - (TIMESTAMP_WINDOW as u64);

        let mut signable = vec![];
        signable.extend_from_slice(signer.public_key().as_bytes());
        signable.extend_from_slice(homeserver.as_bytes());
        signable.extend_from_slice(&postcard::to_allocvec(&capabilities).unwrap());

        let signature = signer.sign(&signable);

        let token = AuthToken {
            version: 0,
            pubky: signer.public_key(),
            homeserver,
            timestamp,
            signature,
            capabilities,
        };

        let serialized = token.serialize();

        let result = verifier.verify(&serialized);

        assert_eq!(result, Err(Error::Expired));
    }

    #[test]
    fn already_used() {
        let signer = Keypair::random();
        let homeserver = Keypair::random().public_key();
        let capabilities = vec![Capability::root()];

        let verifier = AuthVerifier::new(homeserver.clone());

        let token = AuthToken::sign(&signer, &homeserver, capabilities.clone());

        let serialized = &token.serialize();

        verifier.verify(serialized).unwrap();

        assert_eq!(token.capabilities, capabilities);

        assert_eq!(verifier.verify(serialized), Err(Error::AlreadyUsed));
    }
}
