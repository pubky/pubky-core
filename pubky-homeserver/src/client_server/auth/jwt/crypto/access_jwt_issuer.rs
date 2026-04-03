//! Access JWT minting and verification.
//!
//! The homeserver mints short-lived JWTs after verifying a Grant + PoP.
//! The JWT is used as the `Authorization: Bearer` token on every request.
//! Homeserver-only — the SDK only decodes JWTs without verification.

use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    crypto::{Keypair, PublicKey},
};

use super::jws_crypto::{self, JwsCompact};

/// Mint a new Access JWT signed by the homeserver.
pub fn mint_access_jwt(homeserver_keypair: &Keypair, claims: &AccessJwtClaims) -> JwsCompact {
    let header = jws_crypto::eddsa_header("JWT");
    let enc = jws_crypto::encoding_key(homeserver_keypair);
    let token = jsonwebtoken::encode(&header, claims, &enc)
        .expect("invariant: encoding valid claims with a valid key");
    JwsCompact::from_trusted(token)
}

/// Verify an Access JWT against the homeserver's public key.
///
/// Checks:
/// 1. EdDSA signature is valid against the homeserver's public key
/// 2. Token has not expired
pub fn verify_access_jwt(
    compact: &JwsCompact,
    homeserver_pubkey: &PublicKey,
) -> Result<AccessJwtClaims, Error> {
    let claims = verify_signature(compact.as_str(), homeserver_pubkey)?;
    if claims.is_expired(Utc::now().timestamp() as u64) {
        return Err(Error::Expired);
    }
    Ok(claims)
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

/// Errors from Access JWT operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The EdDSA signature does not match the homeserver's public key.
    #[error("invalid JWT signature")]
    InvalidSignature,

    /// The JWT has expired.
    #[error("JWT has expired")]
    Expired,
}

#[cfg(test)]
mod tests {
    use pubky_common::{
        auth::jws::{GrantId, TokenId},
        crypto::Keypair,
    };

    use super::*;

    /// Default JWT lifetime: 1 hour.
    const DEFAULT_JWT_LIFETIME_SECS: u64 = 3600;

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

        let token = mint_access_jwt(&hs_kp, &raw);
        let claims = verify_access_jwt(&token, &hs_kp.public_key()).unwrap();

        assert_eq!(claims.sub, user_kp.public_key());
        assert_eq!(claims.gid, raw.gid);
        assert_eq!(claims.jti, raw.jti);
    }

    #[test]
    fn reject_wrong_homeserver_key() {
        let hs_kp = Keypair::random();
        let wrong_kp = Keypair::random();
        let user_kp = Keypair::random();
        let raw = make_raw_jwt(&hs_kp, &user_kp);

        let token = mint_access_jwt(&hs_kp, &raw);
        let result = verify_access_jwt(&token, &wrong_kp.public_key());
        assert!(matches!(result, Err(Error::InvalidSignature)));
    }

    #[test]
    fn reject_expired_jwt() {
        let hs_kp = Keypair::random();
        let user_kp = Keypair::random();
        let mut raw = make_raw_jwt(&hs_kp, &user_kp);
        raw.exp = 1000; // far in the past

        let token = mint_access_jwt(&hs_kp, &raw);
        let result = verify_access_jwt(&token, &hs_kp.public_key());
        assert!(matches!(result, Err(Error::Expired)));
    }
}
