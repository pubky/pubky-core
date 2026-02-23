//! Client-server Authentication using signed timesteps

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::{
    capabilities::Capabilities,
    crypto::{Keypair, PublicKey, Signature},
    namespaces::PUBKY_AUTH,
    timestamp::Timestamp,
};

const CURRENT_VERSION: u8 = 0;
// 3 minutes in the past or the future
const TIMESTAMP_WINDOW: i64 = 180 * 1_000_000;

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
    ///   the [AuthToken::public_key], without any delegation.
    /// - Capabilities are only meant for resoucres on the homeserver.
    version: u8,
    /// Timestamp
    timestamp: Timestamp,
    /// The [PublicKey] of the owner of the resources being accessed by this token.
    public_key: PublicKey,
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
            public_key: keypair.public_key(),
            capabilities: capabilities.into(),
        };

        let serialized = token.serialize();

        token.signature = keypair.sign(&serialized[65..]);

        token
    }

    // === Getters ===

    /// Returns the public key that is providing this AuthToken
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Returns the capabilities in this AuthToken.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Returns the timestamp of this AuthToken.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
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
                    .public_key
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

    fn signable(version: u8, bytes: &[u8]) -> &[u8] {
        match version {
            0 => bytes[65..].into(),
            _ => unreachable!(),
        }
    }
}

/// Uniquely identifies an [AuthToken] by its timestamp and public key.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenId {
    timestamp: Timestamp,
    public_key: PublicKey,
}

impl Ord for TokenId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.timestamp
            .cmp(&other.timestamp)
            .then_with(|| self.public_key.as_bytes().cmp(other.public_key.as_bytes()))
    }
}

impl PartialOrd for TokenId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Sorted set of [TokenId]s that have already been used.
///
/// Prevents replay attacks by rejecting tokens that were already seen,
/// and periodically garbage-collects entries that are too old to matter.
#[derive(Debug, Clone, Default)]
struct ReplayGuard {
    seen: Vec<TokenId>,
}

impl ReplayGuard {
    /// Record a token and reject it if already seen.
    fn check_and_track(&mut self, id: TokenId) -> Result<(), Error> {
        match self.seen.binary_search(&id) {
            Ok(_) => Err(Error::AlreadyUsed),
            Err(index) => {
                self.seen.insert(index, id);
                Ok(())
            }
        }
    }

    /// Remove entries older than twice the [TIMESTAMP_WINDOW],
    /// since they can never be replayed.
    fn gc(&mut self) {
        let cutoff = Timestamp::now() - 2 * TIMESTAMP_WINDOW as u64;

        let expired_count = self.seen.partition_point(|id| id.timestamp < cutoff);

        self.seen.drain(..expired_count);
    }
}

#[derive(Debug, Clone, Default)]
/// Verifies [AuthToken]s and guards against replay attacks.
pub struct AuthVerifier {
    replay_guard: Arc<Mutex<ReplayGuard>>,
}

impl AuthVerifier {
    /// Verify an [AuthToken] by parsing it from its canonical binary representation,
    /// verifying its signature, and confirm it wasn't already used.
    pub fn verify(&self, bytes: &[u8]) -> Result<AuthToken, Error> {
        let token = AuthToken::verify(bytes)?;

        let id = TokenId {
            timestamp: token.timestamp,
            public_key: token.public_key.clone(),
        };

        let mut guard = self.replay_guard.lock().unwrap();
        guard.gc();
        guard.check_and_track(id)?;

        Ok(token)
    }
}

#[derive(thiserror::Error, Debug, PartialEq, Eq)]
/// Error verifying an [AuthToken]
pub enum Error {
    #[error("Unknown version")]
    /// Unknown version
    UnknownVersion,
    #[error("AuthToken has a timestamp that is more than 3 minutes in the future")]
    /// AuthToken has a timestamp that is more than 3 minutes in the future
    TooFarInTheFuture,
    #[error("AuthToken has a timestamp that is more than 3 minutes in the past")]
    /// AuthToken has a timestamp that is more than 3 minutes in the past
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
            public_key: signer.public_key(),
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

    /// Build a validly signed AuthToken with an arbitrary timestamp.
    fn sign_with_timestamp(signer: &Keypair, timestamp: Timestamp) -> AuthToken {
        let mut token = AuthToken {
            signature: Signature::from_bytes(&[0; 64]),
            namespace: *PUBKY_AUTH,
            version: 0,
            timestamp,
            public_key: signer.public_key(),
            capabilities: Capabilities(vec![Capability::root()]),
        };

        let serialized = token.serialize();
        token.signature = signer.sign(&serialized[65..]);

        token
    }

    #[test]
    fn too_far_in_future() {
        let signer = Keypair::random();
        let verifier = AuthVerifier::default();

        let timestamp = Timestamp::now() + (TIMESTAMP_WINDOW as u64 + 5_000_000);
        let token = sign_with_timestamp(&signer, timestamp);

        assert_eq!(
            verifier.verify(&token.serialize()),
            Err(Error::TooFarInTheFuture)
        );
    }

    #[test]
    fn within_window() {
        let signer = Keypair::random();
        let verifier = AuthVerifier::default();

        // Just inside the past boundary (TIMESTAMP_WINDOW minus 5 seconds)
        let past_token = sign_with_timestamp(
            &signer,
            Timestamp::now() - (TIMESTAMP_WINDOW as u64 - 5_000_000),
        );
        verifier.verify(&past_token.serialize()).unwrap();

        // Just inside the future boundary (TIMESTAMP_WINDOW minus 5 seconds)
        let future_token = sign_with_timestamp(
            &signer,
            Timestamp::now() + (TIMESTAMP_WINDOW as u64 - 5_000_000),
        );
        verifier.verify(&future_token.serialize()).unwrap();
    }

    #[test]
    fn replay_guard_gc() {
        let mut guard = ReplayGuard::default();
        let signer = Keypair::random();
        let now = Timestamp::now();

        // Insert an "old" token ID (well beyond 2x the window)
        let old_id = TokenId {
            timestamp: now - 3 * TIMESTAMP_WINDOW as u64,
            public_key: signer.public_key(),
        };
        guard.check_and_track(old_id).unwrap();

        // Insert a "recent" token ID
        let recent_id = TokenId {
            timestamp: now,
            public_key: signer.public_key(),
        };
        guard.check_and_track(recent_id.clone()).unwrap();

        assert_eq!(guard.seen.len(), 2);

        // GC should remove the old entry but keep the recent one
        guard.gc();

        assert_eq!(guard.seen.len(), 1);
        assert_eq!(guard.seen[0], recent_id);
    }
}
