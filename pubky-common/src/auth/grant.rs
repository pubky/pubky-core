//! Grant token claims type.
//!
//! A Grant is a user-signed JWS that authorizes a client to act on behalf
//! of the user with specific capabilities. This module provides the claims
//! type shared between Ring (signs), SDK (decodes), and homeserver (verifies).

use serde::{Deserialize, Serialize};

use crate::{
    auth::jws::{self, ClientId, GrantId},
    capabilities::Capability,
    crypto::PublicKey,
};

/// Grant JWS claims — the serializable JWT body.
///
/// All public key fields use [`PublicKey`] (serde-transparent → z32 string in JSON).
/// Capabilities use [`Capability`] (serializes as `"/scope:actions"` strings).
///
/// # JSON representation
/// ```json
/// {
///   "iss": "{user_pubky_z32}",
///   "client_id": "franky.pubky.app",
///   "caps": ["/pub/franky/:rw"],
///   "cnf": "{client_pubky_z32}",
///   "jti": "{grant_id}",
///   "iat": 1700000000,
///   "exp": 1731536000
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantClaims {
    /// User public key (grant signer).
    #[serde(with = "crate::auth::jws::pubkey_z32")]
    pub iss: PublicKey,
    /// Application identifier (domain string).
    pub client_id: ClientId,
    /// Authorized capabilities.
    pub caps: Vec<Capability>,
    /// Client public key for Proof-of-Possession.
    #[serde(with = "crate::auth::jws::pubkey_z32")]
    pub cnf: PublicKey,
    /// Grant ID (revocation target).
    pub jti: GrantId,
    /// Issued-at timestamp (Unix seconds).
    pub iat: u64,
    /// Expiry timestamp (Unix seconds).
    pub exp: u64,
}

impl GrantClaims {
    /// Decode a Grant JWS without verifying the signature.
    ///
    /// `compact` is a JWS Compact Serialization string (`header.payload.signature`).
    /// SDK uses this to inspect grant contents and check expiry
    /// without needing the signer's public key.
    pub fn decode(compact: &str) -> Result<Self, jws::Error> {
        jws::decode_jws_payload(compact)
    }
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    use crate::crypto::Keypair;

    use super::*;

    #[test]
    fn grant_claims_serde_roundtrip() {
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();

        let grant = GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: GrantId::generate(),
            iat: 1700000000,
            exp: 1731536000,
        };

        let json = serde_json::to_string(&grant).unwrap();
        let parsed: GrantClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(grant, parsed);
    }

    #[test]
    fn grant_claims_decode_from_jws() {
        let user_kp = Keypair::random();
        let client_kp = Keypair::random();

        let grant = GrantClaims {
            iss: user_kp.public_key(),
            client_id: ClientId::new("test.app").unwrap(),
            caps: vec![Capability::root()],
            cnf: client_kp.public_key(),
            jti: GrantId::generate(),
            iat: 1700000000,
            exp: 1731536000,
        };

        // Construct a fake JWS compact string (header.payload.signature)
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"EdDSA\",\"typ\":\"pubky-grant\"}");
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&grant).unwrap());
        let compact = format!("{}.{}.fakesignature", header, payload);

        let decoded = GrantClaims::decode(&compact).unwrap();
        assert_eq!(decoded, grant);
    }
}
