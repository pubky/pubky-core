//! Access JWT claims type.
//!
//! An Access JWT is a short-lived, homeserver-signed token used as the
//! `Authorization: Bearer` token on every request. This module provides
//! the claims type shared between homeserver (mints/verifies) and
//! SDK (decodes for refresh timing).

use serde::{Deserialize, Serialize};

use crate::{
    crypto::PublicKey,
    auth::jws::{self, GrantId, TokenId},
};

/// Access JWT claims — homeserver-signed, short-lived.
///
/// Capabilities and session metadata live in the homeserver's session cache,
/// NOT in the token — keeping the JWT small and enabling instant revocation.
///
/// # JSON representation
/// ```json
/// {
///   "iss": "{homeserver_pubky_z32}",
///   "sub": "{user_pubky_z32}",
///   "gid": "{grant_id}",
///   "jti": "{token_id}",
///   "iat": 1700000000,
///   "exp": 1700003600
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessJwtClaims {
    /// Homeserver public key (identifies the signer).
    #[serde(with = "crate::auth::jws::pubkey_z32")]
    pub iss: PublicKey,
    /// User public key.
    #[serde(with = "crate::auth::jws::pubkey_z32")]
    pub sub: PublicKey,
    /// Grant ID (revocation target, cold-cache recovery).
    pub gid: GrantId,
    /// Token ID (session cache key).
    pub jti: TokenId,
    /// Issued-at timestamp (Unix seconds).
    pub iat: u64,
    /// Expiry timestamp (Unix seconds).
    pub exp: u64,
}

impl AccessJwtClaims {
    /// Decode an Access JWT without verifying the signature.
    ///
    /// `compact` is a JWS Compact Serialization string (`header.payload.signature`).
    pub fn decode(compact: &str) -> Result<Self, jws::Error> {
        jws::decode_jws_payload(compact)
    }

    /// Check if the token is expired at a given time.
    pub fn is_expired(&self, now: u64) -> bool {
        self.exp <= now
    }
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    use crate::crypto::Keypair;

    use super::*;

    #[test]
    fn access_jwt_claims_serde_roundtrip() {
        let hs_kp = Keypair::random();
        let user_kp = Keypair::random();

        let jwt = AccessJwtClaims {
            iss: hs_kp.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: 1700000000,
            exp: 1700003600,
        };

        let json = serde_json::to_string(&jwt).unwrap();
        let parsed: AccessJwtClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(jwt, parsed);
    }

    #[test]
    fn access_jwt_claims_decode_from_jws() {
        let hs_kp = Keypair::random();
        let user_kp = Keypair::random();

        let jwt = AccessJwtClaims {
            iss: hs_kp.public_key(),
            sub: user_kp.public_key(),
            gid: GrantId::generate(),
            jti: TokenId::generate(),
            iat: 1700000000,
            exp: 1700003600,
        };

        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"EdDSA\",\"typ\":\"JWT\"}");
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&jwt).unwrap());
        let compact = format!("{}.{}.fakesignature", header, payload);

        let decoded = AccessJwtClaims::decode(&compact).unwrap();
        assert_eq!(decoded, jwt);
    }
}
