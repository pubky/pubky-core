use ed25519_dalek::ed25519::SignatureBytes;

use crate::{
    crypto::{Hash, Keypair, PublicKey, Signature},
    error::{Error, Result},
    time::Timestamp,
};

pub const MAX_ATTESTATION_AGE: u64 = 60 * 1000 * 1000;

const BEARER_ATTESTATION_LEN: usize = 144;
const TOKEN_ATTESTATION_LEN: usize = 176;

#[derive(Debug)]
pub enum VerifiedAttestation {
    /// Authenticate the bearer of this attestation as the this [PublicKey].
    Bearer(PublicKey),
    /// Authenticate the bearer of the preimage of this [Hash] as the this [PublicKey].
    ForToken(PublicKey, Hash),
}

/// Homeaudience generated challenge to be signed by a client's private key
/// for authentication (signup / login).
///
/// Encoded as `<32 bytes signer's pubky><64 bytes signature><payload>`
/// where `payload` is encoded as `<32 bytes signer's public_key><64 bytes signature><8 bytes namespace><8 bytes timestamp><audience's public_key>[<32 bytes blake3 hash of an access token>]`
#[derive(Debug, PartialEq)]
pub struct Attestation(Box<[u8]>);

impl Attestation {
    fn new(keypair: Keypair, audience: &PublicKey, access_token: Option<&[u8]>) -> Self {
        let mut bytes = [0u8; TOKEN_ATTESTATION_LEN];

        bytes[..32].copy_from_slice(keypair.public_key().as_bytes());
        bytes[96..104].copy_from_slice(crate::namespaces::PUBKY_AUTH);
        bytes[104..112].copy_from_slice(&Timestamp::now().to_bytes());
        bytes[112..144].copy_from_slice(audience.as_bytes());

        if let Some(access_token) = access_token {
            bytes[144..].copy_from_slice(access_token);
        }

        let signature = keypair
            .sign(
                &bytes[96..if access_token.is_some() {
                    TOKEN_ATTESTATION_LEN
                } else {
                    BEARER_ATTESTATION_LEN
                }],
            )
            .to_bytes();

        bytes[32..96].copy_from_slice(&signature);

        Self(if access_token.is_some() {
            bytes.into()
        } else {
            bytes[0..144].into()
        })
    }

    pub fn bearer(keypair: Keypair, audience: &PublicKey) -> Self {
        Attestation::new(keypair, audience, None)
    }

    pub fn with_token(keypair: Keypair, audience: &PublicKey, token: &[u8]) -> Self {
        Attestation::new(keypair, audience, Some(token))
    }

    pub fn verify(bytes: &[u8], audience: &PublicKey) -> Result<VerifiedAttestation> {
        // TODO: document errors
        if bytes.len() != BEARER_ATTESTATION_LEN && bytes.len() != TOKEN_ATTESTATION_LEN {
            return Err(Error::Generic(format!(
                "Attestation should be 144 or 172 bytes long, got {} bytes instead",
                bytes.len()
            )));
        }

        if &bytes[112..144] != audience.as_bytes() {
            return Err(Error::Generic(
                "Attestation is not meant for the provided audience's public_key".to_string(),
            ));
        }

        // TODO: validate timestamp
        let timestamp = Timestamp::try_from(&bytes[104..112])?;
        let now = Timestamp::now();

        if now.difference(&timestamp) > MAX_ATTESTATION_AGE {
            if timestamp < now {
                return Err(Error::Generic("Attestation is too old".to_string()));
            }

            return Err(Error::Generic(
                "Attestation is too far in the future".to_string(),
            ));
        }

        let public_key = public_key(bytes)?;
        let signature = signature(bytes);

        public_key
            .verify(&bytes[96..], &signature)
            .map_err(|_| Error::Generic("Invalid signature".to_string()))?;

        if bytes.len() == TOKEN_ATTESTATION_LEN {
            let hash: [u8; 32] = bytes[144..].try_into().expect("should not be reachable");

            Ok(VerifiedAttestation::ForToken(
                public_key,
                Hash::from_bytes(hash),
            ))
        } else {
            Ok(VerifiedAttestation::Bearer(public_key))
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

fn public_key(bytes: &[u8]) -> Result<PublicKey> {
    let public_key_bytes: [u8; 32] = bytes[0..32]
        .try_into()
        .expect("Validate attestation length earlier");

    Ok(PublicKey::try_from(&public_key_bytes).map_err(|_| Error::Generic("stuff".to_string()))?)
}

fn signature(bytes: &[u8]) -> Signature {
    let bytes: SignatureBytes = bytes[32..96]
        .try_into()
        .expect("validate token length on instantiating");
    Signature::from(bytes)
}

#[cfg(test)]
mod tests {
    use crate::crypto::Keypair;

    use super::Attestation;

    #[test]
    fn sign_verify() {
        let keypair = Keypair::random();
        let audience = Keypair::random().public_key();

        let attestation = Attestation::bearer(keypair, &audience);

        Attestation::verify(attestation.as_bytes(), &audience).unwrap();

        {
            // Invalid signable
            let mut invalid = attestation.as_bytes().to_vec();
            invalid[96..104].copy_from_slice(&[0; 8]);

            assert!(!Attestation::verify(&invalid, &audience).is_ok())
        }

        {
            // Invalid signer
            let mut invalid = attestation.as_bytes().to_vec();
            invalid[0..32].copy_from_slice(&[0; 32]);

            assert!(!Attestation::verify(&invalid, &audience).is_ok())
        }
    }
}
