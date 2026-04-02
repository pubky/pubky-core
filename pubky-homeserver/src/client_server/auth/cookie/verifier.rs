//! Server-side [AuthToken] verification with replay protection.

use std::sync::{Arc, Mutex};

use pubky_common::{
    auth::{AuthToken, Error},
    crypto::PublicKey,
    timestamp::Timestamp,
};

/// 3 minutes in the past or the future (matching the AuthToken timestamp window).
const TIMESTAMP_WINDOW: i64 = 180 * 1_000_000;

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
            timestamp: token.timestamp(),
            public_key: token.public_key().clone(),
        };

        let mut guard = self.replay_guard.lock().unwrap();
        guard.gc();
        guard.check_and_track(id)?;

        Ok(token)
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::{
        capabilities::Capability,
        crypto::Keypair,
        timestamp::Timestamp,
    };

    use super::*;

    #[test]
    fn sign_and_verify_through_verifier() {
        let signer = Keypair::random();
        let verifier = AuthVerifier::default();

        let token = AuthToken::sign(&signer, vec![Capability::root()]);
        verifier.verify(&token.serialize()).unwrap();
    }

    #[test]
    fn already_used() {
        let signer = Keypair::random();
        let verifier = AuthVerifier::default();

        let token = AuthToken::sign(&signer, vec![Capability::root()]);
        let serialized = token.serialize();

        verifier.verify(&serialized).unwrap();
        assert_eq!(verifier.verify(&serialized), Err(Error::AlreadyUsed));
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
