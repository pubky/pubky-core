//! Grant verification with full Ed25519 signature check.
//!
//! Verifies a Grant JWS compact string, extracting and validating all claims.
//! Homeserver-only — the SDK only decodes grants without verification.
//! 
//! Pubky Ring creates these grants to give the SDKs the necessary information to authenticate and authorize requests to the homeserver.
//! The homesserver verifies the grant and returns a short-lived access token for API calls.

use chrono::{DateTime, Utc};
use pubky_common::{
    capabilities::Capabilities,
    crypto::PublicKey,
    auth::grant::GrantClaims,
    auth::jws::{ClientId, GrantId},
};

use super::jws_crypto::{self, JwsCompact};

/// Verified and parsed grant — the type the homeserver works with.
///
/// All timestamps are converted from JWT Unix seconds to [`DateTime<Utc>`],
/// consistent with the codebase's use of chrono throughout.
#[derive(Clone, Debug)]
pub struct Grant {
    /// User public key (grant signer).
    pub issuer_key: PublicKey,
    /// Client public key for Proof-of-Possession.
    pub cnf_key: PublicKey,
    /// Authorized capabilities.
    pub capabilities: Capabilities,
    /// Application identifier.
    pub client_id: ClientId,
    /// Grant ID (revocation target).
    pub grant_id: GrantId,
    /// When the grant was issued.
    pub issued_at: DateTime<Utc>,
    /// When the grant expires.
    pub expires_at: DateTime<Utc>,
}

impl Grant {
    /// Verify a Grant JWS Compact Serialization string.
    ///
    /// Checks:
    /// 1. Header `typ` is `"pubky-grant"` and `alg` is `EdDSA`
    /// 2. Ed25519 signature is valid against the `iss` public key
    /// 3. Grant has not expired
    /// 4. All required fields are present and valid
    pub fn verify(compact: &JwsCompact) -> Result<Self, Error> {
        let issuer_key = extract_issuer_key(compact.as_str())?;
        let raw = verify_signature(compact.as_str(), &issuer_key)?;
        check_header_type(compact.as_str())?;
        check_expiry(&raw)?;
        parse_verified_grant(raw, issuer_key)
    }
}

/// Extract the `iss` claim from the JWT payload without verifying the signature.
/// Needed because we must know the public key before we can verify.
fn extract_issuer_key(compact: &str) -> Result<PublicKey, Error> {
    let raw = GrantClaims::decode(compact).map_err(|_| Error::InvalidFormat)?;
    Ok(raw.iss)
}

/// Verify the JWS signature against the issuer's public key.
fn verify_signature(compact: &str, issuer_key: &PublicKey) -> Result<GrantClaims, Error> {
    let decoding_key = jws_crypto::decoding_key(issuer_key);
    let validation = jws_crypto::eddsa_validation();
    let token_data = jsonwebtoken::decode::<GrantClaims>(compact, &decoding_key, &validation)
        .map_err(|_| Error::InvalidSignature)?;
    Ok(token_data.claims)
}

/// Check that the JWS header has `typ: "pubky-grant"`.
fn check_header_type(compact: &str) -> Result<(), Error> {
    let header = jsonwebtoken::decode_header(compact).map_err(|_| Error::InvalidFormat)?;
    match header.typ.as_deref() {
        Some("pubky-grant") => Ok(()),
        _ => Err(Error::InvalidHeaderType),
    }
}

/// Check that the grant has not expired.
fn check_expiry(raw: &GrantClaims) -> Result<(), Error> {
    let now = Utc::now().timestamp() as u64;
    if raw.exp <= now {
        return Err(Error::Expired);
    }
    Ok(())
}

/// Convert raw claims into a verified [`Grant`] with parsed types.
fn parse_verified_grant(raw: GrantClaims, issuer_key: PublicKey) -> Result<Grant, Error> {
    let issued_at =
        DateTime::from_timestamp(raw.iat as i64, 0).ok_or(Error::InvalidTimestamp)?;
    let expires_at =
        DateTime::from_timestamp(raw.exp as i64, 0).ok_or(Error::InvalidTimestamp)?;

    Ok(Grant {
        issuer_key,
        cnf_key: raw.cnf,
        capabilities: raw.caps.into(),
        client_id: raw.client_id,
        grant_id: raw.jti,
        issued_at,
        expires_at,
    })
}

/// Errors from Grant verification.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The JWS format is invalid or unparseable.
    #[error("invalid grant format")]
    InvalidFormat,

    /// The JWS header `typ` is not `"pubky-grant"`.
    #[error("invalid grant header type, expected pubky-grant")]
    InvalidHeaderType,

    /// The Ed25519 signature does not match the `iss` public key.
    #[error("invalid grant signature")]
    InvalidSignature,

    /// The grant has expired (`exp` is in the past).
    #[error("grant has expired")]
    Expired,

    /// A timestamp could not be converted to a valid datetime.
    #[error("invalid timestamp in grant")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use pubky_common::{
        capabilities::Capability,
        crypto::Keypair,
        auth::jws::GrantId,
    };

    use super::*;
    use super::jws_crypto;

    fn sign_raw_grant(keypair: &Keypair, raw: &GrantClaims) -> JwsCompact {
        let header = jws_crypto::eddsa_header("pubky-grant");
        let enc = jws_crypto::encoding_key(keypair);
        let token = jsonwebtoken::encode(&header, raw, &enc).unwrap();
        JwsCompact::parse(&token).unwrap()
    }

    fn make_valid_raw_grant(user_kp: &Keypair, client_kp: &Keypair) -> GrantClaims {
        let now = Utc::now().timestamp() as u64;
        GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: GrantId::generate(),
            iat: now,
            exp: now + 3600,
        }
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();
        let raw = make_valid_raw_grant(&user_kp, &client_kp);
        let compact = sign_raw_grant(&user_kp, &raw);

        let grant = Grant::verify(&compact).unwrap();
        assert_eq!(grant.issuer_key, user_kp.public_key());
        assert_eq!(grant.cnf_key, client_kp.public_key());
        assert_eq!(grant.client_id, raw.client_id);
        assert_eq!(grant.grant_id, raw.jti);
    }

    #[test]
    fn reject_wrong_signer() {
        let user_kp = Keypair::random();
        let wrong_kp = Keypair::random();
        let client_kp = Keypair::random();
        let raw = make_valid_raw_grant(&user_kp, &client_kp);

        // Sign with wrong key but claim iss is user_kp
        let compact = sign_raw_grant(&wrong_kp, &raw);
        let result = Grant::verify(&compact);
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn reject_expired_grant() {
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();
        let mut raw = make_valid_raw_grant(&user_kp, &client_kp);
        raw.exp = 1000; // far in the past

        let compact = sign_raw_grant(&user_kp, &raw);
        let result = Grant::verify(&compact);
        assert!(matches!(result, Err(Error::Expired)));
    }

    #[test]
    fn reject_wrong_header_type() {
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();
        let raw = make_valid_raw_grant(&user_kp, &client_kp);

        // Sign with wrong typ header
        let header = jws_crypto::eddsa_header("JWT");
        let enc = jws_crypto::encoding_key(&user_kp);
        let compact = JwsCompact::parse(&jsonwebtoken::encode(&header, &raw, &enc).unwrap()).unwrap();

        let result = Grant::verify(&compact);
        assert!(matches!(result, Err(Error::InvalidHeaderType)));
    }
}
