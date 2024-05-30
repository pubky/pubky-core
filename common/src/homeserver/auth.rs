use ed25519_dalek::ed25519::SignatureBytes;

use crate::{
    crypto::{Hash, Keypair, PublicKey, Signature},
    error::{Error, Result},
    time::Timestamp,
};

// Tolerate 45 seconds in the past (delays)
pub const MAX_AUTHN_SIGNATURE_DIFF: u64 = 90_000_000;

const BEARER_AUTHN_SIGNATURE_LEN: usize = 144;
const TOKEN_AUTHN_SIGNATURE_LEN: usize = 176;

#[derive(Debug)]
pub enum AuthnVerified {
    /// Authenticate the bearer of this authn_signature as the this [PublicKey].
    Bearer(PublicKey),
    /// Authenticate the bearer of the preimage of this [Hash] as the this [PublicKey].
    ForToken(PublicKey, Hash),
}

/// Homeaudience generated challenge to be signed by a client's private key
/// for authentication (signup / login).
///
/// Encoded as `<64 bytes signature><payload>`
/// where `payload` is encoded as `<8 bytes namespace><8 bytes microseconds timestamp (big-endian)><issuer's public_key><audience's public_key>[<32 bytes blake3 hash of an access token>]`
#[derive(Debug, PartialEq)]
pub struct AuthnSignature(Box<[u8]>);

impl AuthnSignature {
    fn new(keypair: Keypair, audience: &PublicKey, access_token: Option<&[u8]>) -> Self {
        let mut bytes = [0u8; TOKEN_AUTHN_SIGNATURE_LEN];

        bytes[64..72].copy_from_slice(crate::namespaces::PK_AUTHN);
        bytes[72..80].copy_from_slice(&Timestamp::now().to_bytes());
        bytes[80..112].copy_from_slice(keypair.public_key().as_bytes());
        bytes[112..BEARER_AUTHN_SIGNATURE_LEN].copy_from_slice(audience.as_bytes());

        if let Some(access_token) = access_token {
            bytes[BEARER_AUTHN_SIGNATURE_LEN..].copy_from_slice(access_token);
        }

        let signature = keypair
            .sign(
                &bytes[64..if access_token.is_some() {
                    TOKEN_AUTHN_SIGNATURE_LEN
                } else {
                    BEARER_AUTHN_SIGNATURE_LEN
                }],
            )
            .to_bytes();

        bytes[0..64].copy_from_slice(&signature);

        Self(if access_token.is_some() {
            bytes.into()
        } else {
            bytes[0..BEARER_AUTHN_SIGNATURE_LEN].into()
        })
    }

    pub fn bearer(keypair: Keypair, audience: &PublicKey) -> Self {
        AuthnSignature::new(keypair, audience, None)
    }

    pub fn with_token(keypair: Keypair, audience: &PublicKey, token: &[u8]) -> Self {
        AuthnSignature::new(keypair, audience, Some(token))
    }

    /// Return the `<timestamp><issuer's public_key>` as a unique time sortable identifier
    pub fn id(&self) -> [u8; 40] {
        self.0[72..112].try_into().unwrap()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn verify(bytes: &[u8], audience: &PublicKey) -> Result<AuthnVerified> {
        // TODO: document errors
        if bytes.len() != BEARER_AUTHN_SIGNATURE_LEN && bytes.len() != TOKEN_AUTHN_SIGNATURE_LEN {
            return Err(Error::Generic(format!(
                "AuthnSignature should be {BEARER_AUTHN_SIGNATURE_LEN} or {TOKEN_AUTHN_SIGNATURE_LEN} bytes long, got {} bytes instead",
                bytes.len()
            )));
        }

        if &bytes[112..BEARER_AUTHN_SIGNATURE_LEN] != audience.as_bytes() {
            return Err(Error::Generic("AUTHN_SIGNATURE_key".to_string()));
        }

        // TODO: validate timestamp
        let timestamp = Timestamp::try_from(&bytes[72..80])?;
        let now = Timestamp::now();

        if now.difference(&timestamp) > MAX_AUTHN_SIGNATURE_DIFF {
            if timestamp < now {
                return Err(Error::Generic("AuthnSignature is too old".to_string()));
            }

            return Err(Error::Generic(
                "AuthnSignature is too far in the future".to_string(),
            ));
        }

        let public_key = public_key(bytes)?;
        let signature = signature(bytes);

        public_key
            .verify(&bytes[64..], &signature)
            .map_err(|_| Error::Generic("Invalid signature".to_string()))?;

        if bytes.len() == TOKEN_AUTHN_SIGNATURE_LEN {
            let hash: [u8; 32] = bytes[BEARER_AUTHN_SIGNATURE_LEN..]
                .try_into()
                .expect("should not be reachable");

            Ok(AuthnVerified::ForToken(public_key, Hash::from_bytes(hash)))
        } else {
            Ok(AuthnVerified::Bearer(public_key))
        }
    }
}

fn public_key(bytes: &[u8]) -> Result<PublicKey> {
    let public_key_bytes: [u8; 32] = bytes[80..112]
        .try_into()
        .expect("Validate authn_signature length earlier");

    Ok(PublicKey::try_from(&public_key_bytes).map_err(|_| Error::Generic("stuff".to_string()))?)
}

fn signature(bytes: &[u8]) -> Signature {
    let bytes: SignatureBytes = bytes[0..64]
        .try_into()
        .expect("validate token length on instantiating");
    Signature::from(bytes)
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::AuthnSignature;

    #[test]
    fn sign_verify() {
        let keypair = Keypair::random();
        let audience = Keypair::random().public_key();

        let authn_signature = AuthnSignature::bearer(keypair, &audience);

        AuthnSignature::verify(authn_signature.as_bytes(), &audience).unwrap();

        {
            // Invalid signable
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[96..104].copy_from_slice(&[0; 8]);

            assert!(!AuthnSignature::verify(&invalid, &audience).is_ok())
        }

        {
            // Invalid signer
            let mut invalid = authn_signature.as_bytes().to_vec();
            invalid[0..32].copy_from_slice(&[0; 32]);

            assert!(!AuthnSignature::verify(&invalid, &audience).is_ok())
        }
    }
}
