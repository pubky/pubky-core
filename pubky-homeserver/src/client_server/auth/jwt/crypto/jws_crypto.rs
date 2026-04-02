//! JWS cryptographic helpers for Ed25519 key conversion.
//!
//! Bridges pubky-common's raw Ed25519 keys to the `jsonwebtoken` crate's
//! DER-encoded format. Homeserver-only — signing and verification happen here.

use std::fmt;

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use pubky_common::crypto::{Keypair, PublicKey};
use serde::{Deserialize, Deserializer};

// ── JWS Compact Serialization ────────────────────────────────────────────────

/// A JWS Compact Serialization string (RFC 7515 §7.1).
///
/// Three base64url-encoded segments separated by dots: `header.payload.signature`.
/// Validated on construction to contain exactly three dot-separated parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JwsCompact(String);

impl JwsCompact {
    /// Parse a string into a [`JwsCompact`], validating the three-part structure.
    pub fn parse(s: &str) -> Result<Self, JwsCompactError> {
        if s.splitn(4, '.').count() != 3 {
            return Err(JwsCompactError);
        }
        Ok(Self(s.to_string()))
    }

    /// Returns the inner string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JwsCompact {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for JwsCompact {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Error returned when a string is not a valid JWS Compact Serialization.
#[derive(Debug)]
pub struct JwsCompactError;

impl fmt::Display for JwsCompactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JWS Compact Serialization must have exactly 3 dot-separated parts")
    }
}

impl std::error::Error for JwsCompactError {}

// ── Key conversion ───────────────────────────────────────────────────────────

/// Fixed ASN.1 prefix for Ed25519 SPKI public keys (RFC 8410).
/// Structure: SEQUENCE { AlgorithmIdentifier { Ed25519 }, BIT STRING { pubkey } }
const ED25519_SPKI_PREFIX: [u8; 12] = [
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

/// Create a `jsonwebtoken` [`EncodingKey`] from a pubky [`Keypair`].
///
/// Uses PEM format to provide both the seed and public key, avoiding
/// potential key derivation mismatches between `ring` and `ed25519-dalek`.
pub fn encoding_key(keypair: &Keypair) -> EncodingKey {
    let pem = ed25519_keypair_to_pem(keypair);
    EncodingKey::from_ed_pem(pem.as_bytes())
        .expect("invariant: PEM is constructed from valid Ed25519 key bytes")
}

/// Create a `jsonwebtoken` [`DecodingKey`] from a pubky [`PublicKey`].
pub fn decoding_key(pubkey: &PublicKey) -> DecodingKey {
    let pem = ed25519_pubkey_to_pem(pubkey.as_bytes());
    DecodingKey::from_ed_pem(pem.as_bytes())
        .expect("invariant: PEM is constructed from valid Ed25519 key bytes")
}

/// Create a JWS header for EdDSA with a custom `typ`.
pub fn eddsa_header(typ: &str) -> Header {
    let mut header = Header::new(Algorithm::EdDSA);
    header.typ = Some(typ.to_string());
    header
}

/// Create a [`Validation`] configured for EdDSA without default claim checks.
///
/// Disables `iss`, `sub`, and `aud` validation — those are checked manually
/// in each verifier with domain-specific logic.
pub fn eddsa_validation() -> Validation {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    validation
}

/// Encode an Ed25519 keypair as PKCS#8 v2 PEM.
///
/// Uses v2 format (seed + public key) to ensure `ring` uses the same public key
/// as `ed25519-dalek`, avoiding any key derivation inconsistencies between libraries.
fn ed25519_keypair_to_pem(keypair: &Keypair) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let seed = keypair.secret();
    let pubkey = keypair.public_key();

    // PKCS#8 v2 DER: version=1, Ed25519 OID, seed, optional public key
    // Content: 3(version) + 7(algid) + 36(privkey) + 37(pubkey) = 83 bytes
    let mut der = Vec::with_capacity(85);
    // SEQUENCE (83 bytes)
    der.extend_from_slice(&[0x30, 0x53]);
    // INTEGER 1 (version v2)
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    // AlgorithmIdentifier { Ed25519 }
    der.extend_from_slice(&[0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70]);
    // OCTET STRING { OCTET STRING { seed } }
    der.extend_from_slice(&[0x04, 0x22, 0x04, 0x20]);
    der.extend_from_slice(&seed);
    // [1] CONSTRUCTED { BIT STRING { pubkey } }
    der.extend_from_slice(&[0xa1, 0x23, 0x03, 0x21, 0x00]);
    der.extend_from_slice(pubkey.as_bytes());

    debug_assert_eq!(der.len(), 85);

    let b64 = STANDARD.encode(&der);
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        b64
    )
}

/// Encode an Ed25519 public key as SPKI PEM.
fn ed25519_pubkey_to_pem(pubkey: &[u8; 32]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let mut der = Vec::with_capacity(ED25519_SPKI_PREFIX.len() + 32);
    der.extend_from_slice(&ED25519_SPKI_PREFIX);
    der.extend_from_slice(pubkey);

    let b64 = STANDARD.encode(&der);
    format!(
        "-----BEGIN PUBLIC KEY-----\n{}\n-----END PUBLIC KEY-----\n",
        b64
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoding_decoding_key_roundtrip() {
        let keypair = Keypair::random();
        let enc = encoding_key(&keypair);
        let dec = decoding_key(&keypair.public_key());

        // Sign and verify a simple payload to confirm keys work
        let header = eddsa_header("JWT");
        let claims = serde_json::json!({"sub": "test", "exp": 9999999999u64});
        let token = jsonwebtoken::encode(&header, &claims, &enc).unwrap();

        let validation = eddsa_validation();
        let decoded: jsonwebtoken::TokenData<serde_json::Value> =
            jsonwebtoken::decode(&token, &dec, &validation).unwrap();

        assert_eq!(decoded.claims["sub"], "test");
    }

    #[test]
    fn wrong_key_fails_verification() {
        let keypair = Keypair::random();
        let wrong_keypair = Keypair::random();

        let enc = encoding_key(&keypair);
        let wrong_dec = decoding_key(&wrong_keypair.public_key());

        let header = eddsa_header("JWT");
        let claims = serde_json::json!({"sub": "test"});
        let token = jsonwebtoken::encode(&header, &claims, &enc).unwrap();

        let validation = eddsa_validation();
        let result =
            jsonwebtoken::decode::<serde_json::Value>(&token, &wrong_dec, &validation);

        assert!(result.is_err());
    }
}
