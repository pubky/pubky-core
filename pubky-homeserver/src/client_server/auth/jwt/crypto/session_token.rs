//! Session bearer token and its storage-side hash.
//!
//! The bearer is 32 random bytes from `OsRng`, base64url-encoded on the
//! wire (~43 chars). The homeserver persists only the SHA-256 hash, so a
//! database leak cannot yield usable bearers.
//!
//! Two newtypes enforce these invariants and carry the behavior:
//! - [`SessionBearer`] — what the client presents in `Authorization: Bearer`.
//! - [`SessionTokenHash`] — the 32-byte digest stored in `grant_sessions.token_hash`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, TryRngCore};
use sha2::{Digest, Sha256};
use std::fmt;

/// Number of random bytes in a freshly generated bearer (256 bits).
const BEARER_RAW_BYTES: usize = 32;

/// Exact length of a valid bearer on the wire.
///
/// base64url-no-pad encoding of 32 bytes is always `ceil(32 * 8 / 6) = 43`
/// chars. Any other length cannot be a bearer we minted.
const BEARER_WIRE_LEN: usize = 43;

// ── SessionBearer ───────────────────────────────────────────────────────────

/// The opaque bearer string presented by the client in `Authorization: Bearer`.
///
/// Constructed either by [`SessionBearer::generate`] (server mint) or
/// [`SessionBearer::parse`] (validating an incoming header). Holding one of
/// these is proof that the string is non-empty and within the length bound.
#[derive(Clone, Debug)]
pub struct SessionBearer(String);

impl SessionBearer {
    /// Mint a new bearer: 32 random bytes from `OsRng`, base64url-encoded.
    pub fn generate() -> Self {
        let mut bytes = [0u8; BEARER_RAW_BYTES];
        OsRng
            .try_fill_bytes(&mut bytes)
            .expect("OsRng must not fail");
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Validate a raw string as a bearer.
    ///
    /// Requires the exact wire length ([`BEARER_WIRE_LEN`]). Content is
    /// otherwise opaque — a correct-length but unknown string is still
    /// caught later by the DB lookup failing.
    pub fn parse(raw: &str) -> Result<Self, SessionBearerError> {
        if raw.len() != BEARER_WIRE_LEN {
            return Err(SessionBearerError::WrongLength {
                actual: raw.len(),
                expected: BEARER_WIRE_LEN,
            });
        }
        Ok(Self(raw.to_string()))
    }

    /// SHA-256 of the bearer's UTF-8 bytes — the value stored in DB.
    pub fn hash(&self) -> SessionTokenHash {
        let mut hasher = Sha256::new();
        hasher.update(self.0.as_bytes());
        SessionTokenHash(hasher.finalize().into())
    }

    /// Borrow the underlying string. Useful for logging and assertions;
    /// production paths prefer [`Self::hash`] or [`Self::into_string`].
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the bearer and return its wire string (for response bodies).
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for SessionBearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Errors produced by [`SessionBearer::parse`].
#[derive(Debug, thiserror::Error)]
pub enum SessionBearerError {
    #[error("bearer token must be {expected} chars (got {actual})")]
    WrongLength { actual: usize, expected: usize },
}

// ── SessionTokenHash ────────────────────────────────────────────────────────

/// SHA-256 of a [`SessionBearer`] — the value indexed in `grant_sessions.token_hash`.
///
/// Constructed via [`SessionBearer::hash`] or [`SessionTokenHash::try_from`] (for
/// database row decoding). 32 bytes, so `Copy` is cheap.
/// We store the hash instead of the bearer itself to mitigate damage from a database 
/// leak: the hash cannot be reversed to yield a valid bearer, and the server 
/// only ever compares hashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionTokenHash([u8; 32]);

impl SessionTokenHash {
    /// Borrow the 32-byte digest as a fixed-size array.
    /// Production paths use [`AsRef<[u8]>`] for sqlx binding.
    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl AsRef<[u8]> for SessionTokenHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl TryFrom<Vec<u8>> for SessionTokenHash {
    type Error = SessionTokenHashError;

    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        <[u8; 32]>::try_from(bytes.as_slice())
            .map(Self)
            .map_err(|_| SessionTokenHashError::WrongLength(bytes.len()))
    }
}

/// Errors produced when decoding a [`SessionTokenHash`] from raw bytes.
#[derive(Debug, thiserror::Error)]
pub enum SessionTokenHashError {
    #[error("session token hash must be 32 bytes, got {0}")]
    WrongLength(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_yields_43_char_base64url() {
        let bearer = SessionBearer::generate();
        assert_eq!(bearer.as_str().len(), 43);
        assert!(bearer
            .as_str()
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn hash_is_deterministic_for_same_wire_string() {
        let bearer = SessionBearer::generate();
        let parsed = SessionBearer::parse(bearer.as_str()).unwrap();
        assert_eq!(bearer.hash(), parsed.hash());
    }

    #[test]
    fn two_generated_bearers_differ() {
        let a = SessionBearer::generate();
        let b = SessionBearer::generate();
        assert_ne!(a.as_str(), b.as_str());
    }

    #[test]
    fn parse_rejects_empty() {
        match SessionBearer::parse("") {
            Err(SessionBearerError::WrongLength { actual, expected }) => {
                assert_eq!(actual, 0);
                assert_eq!(expected, BEARER_WIRE_LEN);
            }
            other => panic!("expected WrongLength, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_too_short() {
        let short = "a".repeat(BEARER_WIRE_LEN - 1);
        match SessionBearer::parse(&short) {
            Err(SessionBearerError::WrongLength { actual, expected }) => {
                assert_eq!(actual, BEARER_WIRE_LEN - 1);
                assert_eq!(expected, BEARER_WIRE_LEN);
            }
            other => panic!("expected WrongLength, got {other:?}"),
        }
    }

    #[test]
    fn parse_rejects_too_long() {
        let huge = "a".repeat(BEARER_WIRE_LEN + 1);
        match SessionBearer::parse(&huge) {
            Err(SessionBearerError::WrongLength { actual, expected }) => {
                assert_eq!(actual, BEARER_WIRE_LEN + 1);
                assert_eq!(expected, BEARER_WIRE_LEN);
            }
            other => panic!("expected WrongLength, got {other:?}"),
        }
    }

    #[test]
    fn parse_accepts_exact_wire_length() {
        let ok = "a".repeat(BEARER_WIRE_LEN);
        assert!(SessionBearer::parse(&ok).is_ok());
    }

    #[test]
    fn generated_bearer_roundtrips_through_parse() {
        let bearer = SessionBearer::generate();
        assert!(SessionBearer::parse(bearer.as_str()).is_ok());
    }

    #[test]
    fn token_hash_try_from_wrong_length_errors() {
        match SessionTokenHash::try_from(vec![0u8; 31]) {
            Err(SessionTokenHashError::WrongLength(n)) => assert_eq!(n, 31),
            other => panic!("expected WrongLength(31), got {other:?}"),
        }
        match SessionTokenHash::try_from(Vec::<u8>::new()) {
            Err(SessionTokenHashError::WrongLength(n)) => assert_eq!(n, 0),
            other => panic!("expected WrongLength(0), got {other:?}"),
        }
    }

    #[test]
    fn token_hash_try_from_roundtrip() {
        let bearer = SessionBearer::generate();
        let hash = bearer.hash();
        let reconstructed = SessionTokenHash::try_from(hash.as_bytes().to_vec()).unwrap();
        assert_eq!(hash, reconstructed);
    }
}
