//! Access JWT minting and verification.
//!
//! The homeserver mints short-lived JWTs after verifying a Grant + PoP.
//! The JWT is used as the `Authorization: Bearer` token on every request.
//! Homeserver-only — the SDK only decodes JWTs without verification.

use chrono::{DateTime, Utc};
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::jws::{GrantId, TokenId},
    crypto::{Keypair, PublicKey},
};

use super::jws_crypto::{self, JwsCompact};

/// Verified access JWT — the type the homeserver works with after verification.
#[derive(Clone, Debug)]
pub struct AccessJwt {
    /// User public key.
    pub user_key: PublicKey,
    /// Grant ID (for revocation checks and cold-cache recovery).
    pub grant_id: GrantId,
    /// Token ID (session cache key).
    pub token_id: TokenId,
    /// When the token was issued.
    pub issued_at: DateTime<Utc>,
    /// When the token expires.
    pub expires_at: DateTime<Utc>,
}

impl AccessJwt {
    /// Mint a new Access JWT signed by the homeserver.
    pub fn mint(homeserver_keypair: &Keypair, raw: &AccessJwtClaims) -> JwsCompact {
        let header = jws_crypto::eddsa_header("JWT");
        let enc = jws_crypto::encoding_key(homeserver_keypair);
        let token = jsonwebtoken::encode(&header, raw, &enc)
            .expect("invariant: encoding valid claims with a valid key");
        JwsCompact::from_trusted(token)
    }

    /// Verify an Access JWT against the homeserver's public key.
    ///
    /// Checks:
    /// 1. EdDSA signature is valid against the homeserver's public key
    /// 2. Token has not expired
    pub fn verify(compact: &JwsCompact, homeserver_pubkey: &PublicKey) -> Result<Self, Error> {
        let raw = verify_signature(compact.as_str(), homeserver_pubkey)?;
        if raw.is_expired(Utc::now().timestamp() as u64) {
            return Err(Error::Expired);
        };
        parse_verified_jwt(raw)
    }
}

fn verify_signature(
    compact: &str,
    homeserver_pubkey: &PublicKey,
) -> Result<AccessJwtClaims, Error> {
    let decoding_key = jws_crypto::decoding_key(homeserver_pubkey);
    let validation = jws_crypto::eddsa_validation();
    let token_data = jsonwebtoken::decode::<AccessJwtClaims>(compact, &decoding_key, &validation)
        .map_err(|_| Error::InvalidSignature)?;
    Ok(token_data.claims)
}

fn parse_verified_jwt(raw: AccessJwtClaims) -> Result<AccessJwt, Error> {
    let issued_at = DateTime::from_timestamp(raw.iat as i64, 0).ok_or(Error::InvalidTimestamp)?;
    let expires_at = DateTime::from_timestamp(raw.exp as i64, 0).ok_or(Error::InvalidTimestamp)?;

    Ok(AccessJwt {
        user_key: raw.sub,
        grant_id: raw.gid,
        token_id: raw.jti,
        issued_at,
        expires_at,
    })
}

/// Errors from Access JWT operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The EdDSA signature does not match the homeserver's public key.
    #[error("invalid JWT signature")]
    InvalidSignature,

    /// The JWT has expired.
    #[error("JWT has expired")]
    Expired,

    /// A timestamp could not be converted to a valid datetime.
    #[error("invalid timestamp in JWT")]
    InvalidTimestamp,
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    /// Default JWT lifetime: 1 hour.
    const DEFAULT_JWT_LIFETIME_SECS: u64 = 3600;

    use super::*;

    fn make_raw_jwt(hs_kp: &Keypair, user_kp: &Keypair) -> AccessJwtClaims {
        let now = Utc::now().timestamp() as u64;
        AccessJwtClaims {
            iss: hs_kp.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: now,
            exp: now + DEFAULT_JWT_LIFETIME_SECS,
        }
    }

    #[test]
    fn mint_and_verify_roundtrip() {
        let hs_kp = Keypair::random();
        let user_kp = Keypair::random();
        let raw = make_raw_jwt(&hs_kp, &user_kp);

        let token = AccessJwt::mint(&hs_kp, &raw);
        let jwt = AccessJwt::verify(&token, &hs_kp.public_key()).unwrap();

        assert_eq!(jwt.user_key, user_kp.public_key());
        assert_eq!(jwt.grant_id, raw.gid);
        assert_eq!(jwt.token_id, raw.jti);
    }

    #[test]
    fn reject_wrong_homeserver_key() {
        let hs_kp = Keypair::random();
        let wrong_kp = Keypair::random();
        let user_kp = Keypair::random();
        let raw = make_raw_jwt(&hs_kp, &user_kp);

        let token = AccessJwt::mint(&hs_kp, &raw);
        let result = AccessJwt::verify(&token, &wrong_kp.public_key());
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn reject_expired_jwt() {
        let hs_kp = Keypair::random();
        let user_kp = Keypair::random();
        let mut raw = make_raw_jwt(&hs_kp, &user_kp);
        raw.exp = 1000; // far in the past

        let token = AccessJwt::mint(&hs_kp, &raw);
        let result = AccessJwt::verify(&token, &hs_kp.public_key());
        assert!(matches!(result, Err(Error::Expired)));
    }
}
