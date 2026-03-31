//! Proof-of-Possession (PoP) verification.
//!
//! Every Grant is bound to a client-controlled cryptographic key. The PoP
//! proof demonstrates the client possesses that key. This prevents Grant
//! replay by homeservers and third-party attackers.
//!
//! Entirely homeserver-side for Phase 1. The SDK will create PoP proofs
//! in Phase 2.

use chrono::{DateTime, Utc};
use pubky_common::{
    auth::jws::{GrantId, PopNonce},
    crypto::PublicKey,
};
use serde::{Deserialize, Serialize};

use super::jws_crypto::{self, JwsCompact};

/// ±3 minutes — matches existing `AuthToken` `TIMESTAMP_WINDOW` in `pubky-common/src/auth.rs`.
pub const POP_MAX_AGE_SECS: u64 = 180;

/// Nonce GC threshold: 2x the PoP window to cover edge cases.
pub const POP_NONCE_GC_THRESHOLD_SECS: u64 = 2 * POP_MAX_AGE_SECS;

/// PoP proof JWS claims — the serializable payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PopProofClaims {
    /// Target homeserver public key — prevents cross-homeserver replay.
    #[serde(with = "pubky_common::auth::jws::pubkey_z32")]
    pub aud: PublicKey,
    /// Grant ID — binds proof to a specific Grant.
    pub gid: GrantId,
    /// Random value — prevents replay within time window.
    pub nonce: PopNonce,
    /// Issued-at timestamp (Unix seconds).
    pub iat: u64,
}

/// Verified PoP proof.
#[derive(Clone, Debug)]
pub struct PopProof {
    /// Grant ID this proof is bound to.
    pub grant_id: GrantId,
    /// The nonce (needed for replay tracking in DB).
    pub nonce: PopNonce,
    /// When the proof was issued.
    pub issued_at: DateTime<Utc>,
}

/// Context required to verify a PoP proof.
///
/// Bundled into a struct to keep `PopProof::verify` at ≤3 parameters.
pub struct PopVerificationContext<'a> {
    /// The client's public key from the Grant's `cnf` claim.
    pub cnf_key: &'a PublicKey,
    /// The homeserver's own public key z32 string.
    pub expected_audience: &'a str,
    /// The Grant's `jti` — the PoP's `gid` must match.
    pub expected_grant_id: &'a GrantId,
}

impl PopProof {
    /// Verify a PoP proof JWS Compact Serialization string.
    ///
    /// Checks:
    /// 1. Ed25519 signature is valid against `cnf_key`
    /// 2. `aud` matches the expected homeserver
    /// 3. `iat` is within the ±3 minute window
    ///
    /// Nonce replay checking is done separately via the database.
    pub fn verify(compact: &JwsCompact, context: &PopVerificationContext) -> Result<Self, Error> {
        let raw = verify_signature(compact.as_str(), context.cnf_key)?;
        check_audience(&raw, context.expected_audience)?;
        check_grant_binding(&raw, context.expected_grant_id)?;
        check_timestamp(&raw)?;
        parse_verified_pop(raw)
    }
}

fn verify_signature(compact: &str, cnf_key: &PublicKey) -> Result<PopProofClaims, Error> {
    let decoding_key = jws_crypto::decoding_key(cnf_key);
    let validation = jws_crypto::eddsa_validation();
    let token_data = jsonwebtoken::decode::<PopProofClaims>(compact, &decoding_key, &validation)
        .map_err(|_| Error::InvalidSignature)?;
    Ok(token_data.claims)
}

fn check_audience(raw: &PopProofClaims, expected: &str) -> Result<(), Error> {
    if raw.aud.z32() != expected {
        return Err(Error::AudienceMismatch);
    }
    Ok(())
}

fn check_grant_binding(raw: &PopProofClaims, expected: &GrantId) -> Result<(), Error> {
    if raw.gid != *expected {
        return Err(Error::GrantIdMismatch);
    }
    Ok(())
}

fn check_timestamp(raw: &PopProofClaims) -> Result<(), Error> {
    let now = Utc::now().timestamp() as u64;
    let diff = if now >= raw.iat {
        now - raw.iat
    } else {
        raw.iat - now
    };
    if diff > POP_MAX_AGE_SECS {
        return Err(Error::TimestampOutOfRange);
    }
    Ok(())
}

fn parse_verified_pop(raw: PopProofClaims) -> Result<PopProof, Error> {
    let issued_at =
        DateTime::from_timestamp(raw.iat as i64, 0).ok_or(Error::InvalidTimestamp)?;
    Ok(PopProof {
        grant_id: raw.gid,
        nonce: raw.nonce,
        issued_at,
    })
}

/// Errors from PoP proof verification.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The Ed25519 signature does not match the `cnf` key.
    #[error("invalid PoP signature")]
    InvalidSignature,

    /// The `aud` claim does not match this homeserver.
    #[error("PoP audience mismatch")]
    AudienceMismatch,

    /// The `gid` claim does not match the Grant's `jti`.
    #[error("PoP grant ID mismatch")]
    GrantIdMismatch,

    /// The `iat` timestamp is outside the ±3 minute window.
    #[error("PoP timestamp out of range")]
    TimestampOutOfRange,

    /// A timestamp could not be converted to a valid datetime.
    #[error("invalid timestamp in PoP proof")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use super::*;
    use super::jws_crypto;

    fn sign_pop(client_kp: &Keypair, raw: &PopProofClaims) -> JwsCompact {
        let header = jws_crypto::eddsa_header("pubky-pop");
        let enc = jws_crypto::encoding_key(client_kp);
        let token = jsonwebtoken::encode(&header, raw, &enc).unwrap();
        JwsCompact::parse(&token).unwrap()
    }

    fn make_valid_pop(hs_kp: &Keypair) -> PopProofClaims {
        PopProofClaims {
            aud: hs_kp.public_key(),
            gid: GrantId::generate(),
            nonce: PopNonce::generate(),
            iat: Utc::now().timestamp() as u64,
        }
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let client_kp = Keypair::random();
        let hs_kp = Keypair::random();
        let raw = make_valid_pop(&hs_kp);
        let compact = sign_pop(&client_kp, &raw);

        let cnf_key = client_kp.public_key();
        let aud = hs_kp.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &cnf_key,
            expected_audience: &aud,
            expected_grant_id: &raw.gid,
        };

        let pop = PopProof::verify(&compact, &context).unwrap();
        assert_eq!(pop.grant_id, raw.gid);
        assert_eq!(pop.nonce, raw.nonce);
    }

    #[test]
    fn reject_wrong_cnf_key() {
        let client_kp = Keypair::random();
        let wrong_kp = Keypair::random();
        let hs_kp = Keypair::random();
        let raw = make_valid_pop(&hs_kp);
        let compact = sign_pop(&client_kp, &raw);

        let wrong_pk = wrong_kp.public_key();
        let aud = hs_kp.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &wrong_pk,
            expected_audience: &aud,
            expected_grant_id: &raw.gid,
        };

        let result = PopProof::verify(&compact, &context);
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn reject_wrong_audience() {
        let client_kp = Keypair::random();
        let hs_kp = Keypair::random();
        let raw = make_valid_pop(&hs_kp);
        let compact = sign_pop(&client_kp, &raw);

        let cnf_key = client_kp.public_key();
        let context = PopVerificationContext {
            cnf_key: &cnf_key,
            expected_audience: "wrong-audience",
            expected_grant_id: &raw.gid,
        };

        let result = PopProof::verify(&compact, &context);
        assert!(matches!(result, Err(Error::AudienceMismatch)));
    }

    #[test]
    fn reject_wrong_grant_id() {
        let client_kp = Keypair::random();
        let hs_kp = Keypair::random();
        let raw = make_valid_pop(&hs_kp);
        let compact = sign_pop(&client_kp, &raw);

        let cnf_key = client_kp.public_key();
        let aud = hs_kp.public_key().z32();
        let wrong_gid = GrantId::generate();
        let context = PopVerificationContext {
            cnf_key: &cnf_key,
            expected_audience: &aud,
            expected_grant_id: &wrong_gid,
        };

        let result = PopProof::verify(&compact, &context);
        assert!(matches!(result, Err(Error::GrantIdMismatch)));
    }

    #[test]
    fn reject_stale_timestamp() {
        let client_kp = Keypair::random();
        let hs_kp = Keypair::random();
        let mut raw = make_valid_pop(&hs_kp);
        raw.iat = 1000; // far in the past
        let compact = sign_pop(&client_kp, &raw);

        let cnf_key = client_kp.public_key();
        let aud = hs_kp.public_key().z32();
        let context = PopVerificationContext {
            cnf_key: &cnf_key,
            expected_audience: &aud,
            expected_grant_id: &raw.gid,
        };

        let result = PopProof::verify(&compact, &context);
        assert!(matches!(result, Err(Error::TimestampOutOfRange)));
    }
}
