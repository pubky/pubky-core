//! Shared JWS decoding utilities and token identifier types.
//!
//! This module provides:
//! - Lightweight JWS payload decoding (no signature verification)
//! - Typed identifiers for grants, tokens, nonces, and client IDs

use std::fmt;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};

use crate::crypto::random_bytes;

/// Maximum length for a [`RandomId`] (supports base64url and UUIDs).
const RANDOM_ID_MAX_LENGTH: usize = 36;

/// Maximum length for a [`ClientId`] (DNS domain name limit per RFC 1035).
const CLIENT_ID_MAX_LENGTH: usize = 253;

// ── JWS Decoding ────────────────────────────────────────────────────────────

/// Decode a JWS Compact Serialization string's payload WITHOUT signature verification.
///
/// `compact` is a JWS in Compact Serialization form (RFC 7515 §7.1):
/// three base64url-encoded segments separated by dots (`header.payload.signature`).
///
/// Splits on `.`, base64url-decodes the payload (second) segment,
/// and deserializes from JSON. Useful for the SDK to inspect token
/// contents and check expiry without needing the signer's public key.
pub fn decode_jws_payload<T: serde::de::DeserializeOwned>(compact: &str) -> Result<T, Error> {
    let parts: Vec<&str> = compact.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(Error::InvalidFormat(
            "JWS compact must have 3 dot-separated parts",
        ));
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| Error::InvalidFormat("invalid base64url in JWS payload"))?;

    serde_json::from_slice(&payload_bytes).map_err(|e| Error::JsonParse(e.to_string()))
}

// ── RandomId ────────────────────────────────────────────────────────────────

/// A cryptographically random identifier, max 36 characters.
///
/// Supports base64url (22 chars for 128-bit) and UUIDs (36 chars with hyphens).
/// Serde-transparent: serializes as a plain string in JSON.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RandomId(String);

impl RandomId {
    /// Generate a new random ID: 128-bit random bytes → base64url (22 chars).
    pub fn generate() -> Self {
        let bytes = random_bytes::<16>();
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Parse and validate an existing ID string.
    ///
    /// Must be non-empty and at most 36 characters.
    pub fn parse(s: &str) -> Result<Self, Error> {
        if s.is_empty() {
            return Err(Error::InvalidFormat("RandomId must not be empty"));
        }
        if s.len() > RANDOM_ID_MAX_LENGTH {
            return Err(Error::InvalidFormat(
                "RandomId must be at most 36 characters",
            ));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the inner string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RandomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RandomId {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<RandomId> for String {
    fn from(id: RandomId) -> Self {
        id.0
    }
}

/// Grant identifier — a [`RandomId`] used as the `jti` claim in a Grant JWS.
pub type GrantId = RandomId;

/// JWT token identifier — a [`RandomId`] used as the `jti` claim in an Access JWT.
pub type TokenId = RandomId;

/// Proof-of-Possession nonce — a [`RandomId`] used to prevent PoP replay.
pub type PopNonce = RandomId;

// ── ClientId ────────────────────────────────────────────────────────────────

/// An application identifier, typically a domain string (e.g., `franky.pubky.app`).
///
/// Max 253 characters (DNS domain name limit per RFC 1035).
/// Serde-transparent: serializes as a plain string in JSON.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ClientId(String);

impl ClientId {
    /// Create a new [`ClientId`], validating that it is non-empty and at most 253 characters.
    pub fn new(s: &str) -> Result<Self, Error> {
        if s.is_empty() {
            return Err(Error::InvalidFormat("ClientId must not be empty"));
        }
        if s.len() > CLIENT_ID_MAX_LENGTH {
            return Err(Error::InvalidFormat(
                "ClientId must be at most 253 characters",
            ));
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the inner string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ClientId {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(&s)
    }
}

impl From<ClientId> for String {
    fn from(id: ClientId) -> Self {
        id.0
    }
}

// ── PublicKey serde as z32 string ────────────────────────────────────────────

/// Serde helper for serializing/deserializing [`PublicKey`] as a z32 string in JSON.
///
/// Use with `#[serde(with = "pubky_common::auth::jws::pubkey_z32")]` on struct fields.
/// This is needed because `PublicKey`'s default serde (via ed25519-dalek) serializes
/// as raw bytes, but JWT payloads need human-readable z32 strings.
pub mod pubkey_z32 {
    use crate::crypto::PublicKey;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a [`PublicKey`] as a z32 string.
    pub fn serialize<S: Serializer>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&key.z32())
    }

    /// Deserialize a [`PublicKey`] from a z32 string.
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PublicKey, D::Error> {
        let s = String::deserialize(deserializer)?;
        PublicKey::try_from_z32(&s).map_err(serde::de::Error::custom)
    }
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors from JWS decoding and ID parsing.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The input format is invalid.
    #[error("{0}")]
    InvalidFormat(&'static str),

    /// JSON parsing failed.
    #[error("JSON parse error: {0}")]
    JsonParse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_id_generate_is_valid() {
        let id = RandomId::generate();
        assert!(!id.as_str().is_empty());
        assert!(id.as_str().len() <= RANDOM_ID_MAX_LENGTH);
        // base64url of 16 bytes = 22 chars
        assert_eq!(id.as_str().len(), 22);
    }

    #[test]
    fn random_id_uniqueness() {
        let a = RandomId::generate();
        let b = RandomId::generate();
        assert_ne!(a, b);
    }

    #[test]
    fn random_id_parse_valid() {
        RandomId::parse("abc123").unwrap();
        RandomId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap(); // UUID
        RandomId::parse("a").unwrap(); // min length
    }

    #[test]
    fn random_id_parse_rejects_empty() {
        assert!(RandomId::parse("").is_err());
    }

    #[test]
    fn random_id_parse_rejects_too_long() {
        let long = "a".repeat(RANDOM_ID_MAX_LENGTH + 1);
        assert!(RandomId::parse(&long).is_err());
    }

    #[test]
    fn random_id_serde_roundtrip() {
        let id = RandomId::generate();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: RandomId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn client_id_valid() {
        ClientId::new("franky.pubky.app").unwrap();
        ClientId::new("a").unwrap();
    }

    #[test]
    fn client_id_rejects_empty() {
        assert!(ClientId::new("").is_err());
    }

    #[test]
    fn client_id_rejects_too_long() {
        let long = "a".repeat(CLIENT_ID_MAX_LENGTH + 1);
        assert!(ClientId::new(&long).is_err());
    }

    #[test]
    fn client_id_serde_roundtrip() {
        let id = ClientId::new("test.app").unwrap();
        let json = serde_json::to_string(&id).unwrap();
        let parsed: ClientId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn decode_jws_payload_valid() {
        // Manually construct a JWS-like string: header.payload.signature
        // Payload: {"sub":"hello"}
        let payload = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"hello\"}");
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"EdDSA\"}");
        let compact = format!("{}.{}.fakesig", header, payload);

        #[derive(Deserialize)]
        struct Claims {
            sub: String,
        }

        let claims: Claims = decode_jws_payload(&compact).unwrap();
        assert_eq!(claims.sub, "hello");
    }

    #[test]
    fn decode_jws_payload_rejects_malformed() {
        assert!(decode_jws_payload::<serde_json::Value>("not.a.valid.jws.toomanyparts").is_err());
        assert!(decode_jws_payload::<serde_json::Value>("only-one-part").is_err());
    }
}
