//! JWT service for the homeserver.
//! 
//! This service is responsible for creating and verifying JWT tokens.
//! It uses the ES256 algorithm for signing and verifying tokens.
//! 
//! The JWT tokens are used to authenticate requests to the homeserver.
//! 
//! https://jwt.io/ is a great resource for debugging and understanding JWT tokens.
//! 

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, TokenData};
use pkarr::PublicKey;
use pubky_common::capabilities::Capability;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet, fmt::Debug, time::{Duration, SystemTime, UNIX_EPOCH}
};
use uuid::Uuid;

use crate::{persistence::lmdb::tables::sessions::SessionId, ES256KeyPair};

use super::{JwtToken, Z32PublicKey};


/// Claims for the JWT token, using standard JWT naming: 'sub' for subject (user_id), 'iss' for issuer (homeserver_pubkey), 'exp' for expiration, 'iat' for issued at, 'jti' for JWT ID.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Claims {
    /// The user's public key.
    pub sub: Z32PublicKey,
    /// The homeserver's public key.
    pub iss: Z32PublicKey,
    /// The expiration time of the token as unix timestamp (seconds since epoch).
    pub exp: usize,
    /// The issued at time of the token as unix timestamp (seconds since epoch).
    pub iat: usize,
    /// The JWT ID. Random 16 bytes base32 encoded.
    pub jti: SessionId,
    /// The capabilities of the token.
    pub capabilities: Vec<Capability>,
}



/// JWT service for the homeserver.
/// 
/// This service is responsible for creating and verifying JWT tokens.
/// It uses the ES256 algorithm for signing and verifying tokens.
/// 
/// The JWT tokens are used to authenticate requests to the homeserver.
/// 
/// https://jwt.io/ is a great resource for debugging and understanding JWT tokens.
/// 
/// Derives the signing keys from the main homeserver keypair.
/// 
#[derive(Clone)]
pub(crate) struct JwtService {
    /// The signing key for the JWT token.
    encoding_key: EncodingKey,
    /// The verifying key for the JWT token.
    decoding_key: DecodingKey,
    /// The public key of the homeserver.
    /// This is added to the token as 'iss' (issuer).
    issuer_pubkey: PublicKey,
}

impl Debug for JwtService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JwtService")
    }
}

impl JwtService {
    /// Create a new JWT service from the main homeserver keypair.
    pub fn new(keypair: &pkarr::Keypair) -> anyhow::Result<Self> {
        let key = ES256KeyPair::derive_from_main_secret_key(&keypair.secret_key())
            .map_err(|e| anyhow::anyhow!("Failed to derive jwt key from pkarr keypair: {}", e))?;


        let encoding_key = EncodingKey::from_ec_pem(key.private_key_pem()?.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to create encoding key: {}", e))?;
        let decoding_key = DecodingKey::from_ec_pem(key.public_key_pem()?.as_bytes())
            .map_err(|e| anyhow::anyhow!("Failed to create decoding key: {}", e))?;
        Ok(Self {
            encoding_key,
            decoding_key,
            issuer_pubkey: keypair.public_key(),
        })
    }

    /// Creates a JWT token for the given user and capabilities.
    ///
    /// # Arguments
    /// * `user_pubkey` - The user's public key (used as 'sub').
    /// * `homeserver_pubkey` - The homeserver's public key (used as 'hs').
    /// * `capabilities` - The list of capabilities to encode.
    ///
    /// # Returns
    /// A signed JWT token as a String.
    pub fn create_token(
        &self,
        user_pubkey: &PublicKey,
        capabilities: &[Capability],
        expires_after: Duration,
        session_id: SessionId,
    ) -> Result<JwtToken, jsonwebtoken::errors::Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as usize;
        let exp = now + expires_after.as_secs() as usize;
        let claims = Claims {
            sub: Z32PublicKey(user_pubkey.clone()),
            iss: Z32PublicKey(self.issuer_pubkey.clone()),
            exp,
            iat: now,
            jti: session_id,
            capabilities: capabilities.to_vec(),
        };

        let header = Header::new(Algorithm::ES256);
        let token = jsonwebtoken::encode(&header, &claims, &self.encoding_key)?;
        JwtToken::new(token)
    }

    /// Validates a JWT token and returns the claims.
    /// 
    /// # Arguments
    /// * `token` - The JWT token to validate.
    /// 
    /// # Returns
    /// The claims of the JWT token.
    /// Will return an error if the token is expired or invalid.
    pub fn validate_token(&self, token: &str) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
        let mut validation = jsonwebtoken::Validation::new(Algorithm::ES256);
        validation.required_spec_claims.insert("sub".to_string());
        validation.required_spec_claims.insert("hs".to_string());
        validation.required_spec_claims.insert("exp".to_string());
        validation.required_spec_claims.insert("capabilities".to_string());
        validation.iss = Some(HashSet::from([self.issuer_pubkey.to_string()]));
        println!("validation: {:?}", validation);
        jsonwebtoken::decode::<Claims>(
            token,
            &self.decoding_key,
            &validation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_service() -> JwtService {
        // Service with the private key:
        //
        // -----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgo6sJpORWZC1Z/gGM\nwRNHEDGskAgU3Tf1c52lDi5QkYehRANCAATu8ZS9A3Eer1B1tFjTyGwQxh2sDBVG\nx3V+ycvAw97UZ1PpiU1J6cRsuiugmPcgLzKIDU46U5wFzATLHDgNT/+C\n-----END PRIVATE KEY-----\n
        //
        // The public key is:
        // -----BEGIN PUBLIC KEY-----\nMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE7vGUvQNxHq9QdbRY08hsEMYdrAwV\nRsd1fsnLwMPe1GdT6YlNSenEbLoroJj3IC8yiA1OOlOcBcwEyxw4DU//gg==\n-----END PUBLIC KEY-----\n
        let secret: [u8; 32] = [0; 32];
        let hs_keypair = pkarr::Keypair::from_secret_key(&secret);
        let service = JwtService::new(&hs_keypair).unwrap();
        service
    }

    #[test]
    fn test_create_jwt_token() {
        let service = create_service();
        let user_keypair = pkarr::Keypair::random();

        let expires_after = Duration::from_secs(10);
        let capabilities = vec![];
        let session_id = SessionId::random();
        let jwt_token = service.create_token(&user_keypair.public_key(), &capabilities, expires_after, session_id.clone()).unwrap();
        let validated_token = service.validate_token(&jwt_token).unwrap();
        assert_eq!(validated_token.claims.sub.0, user_keypair.public_key());
        assert_eq!(validated_token.claims.iss.0, service.issuer_pubkey); 
        assert_eq!(validated_token.claims.capabilities.len(), 0);
        assert_eq!(validated_token.claims.jti, session_id);
    }

    #[test]
    fn test_validate_expired_token() {
        let service = create_service();
        let expired_token = "eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJudDRtbXFuZXB5OWlwYmV6M3Nmc3J0amtmcHNtZjZ5dXFhdW1xdTh0aWVqZ2pneXdhNXVvIiwiaXNzIjoiOHBpbnh4Z3FzNDFuNGFpZGlkZW53NWFwcXAxdXJmbXpkenRyOGp0NGFicmtkbjQzNWV3byIsImV4cCI6MTc0NjUyODA0MywiaWF0IjoxNzQ2NTI4MDMzLCJqdGkiOiJlM2E2ZmQxZi01M2I1LTQ4ODMtYjc2Yi01MDgzNzVhMjkzMTgiLCJjYXBhYmlsaXRpZXMiOltdfQ.SCnQqWnaKn08MUTE8YWclENMvJWt7TS1-4MnwY7KnqJ1Pmrxntqbx3xg77Cdh196CIbMviEPcSYsUr1dPP8_eg";

        let result = service.validate_token(&expired_token);
        let error = result.err().expect("Token should be expired");
        if let jsonwebtoken::errors::ErrorKind::ExpiredSignature = error.kind() {
            // all good
        } else {
            println!("error: {:?}", error.kind());
            panic!("Error should be expired token");
        }
    }
}
