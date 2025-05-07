use std::ops::Deref;

use jsonwebtoken::{DecodingKey, TokenData};

use super::Claims;

/// A simple wrapper around a JWT token that also holds the decoded claims.
/// 
/// This is useful to avoid decoding the token multiple times.
/// 
/// Only valid for our own JWT tokens.
pub struct JwtToken{
    token: String,
    decoded: TokenData<Claims>
}

impl Deref for JwtToken {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.token
    }
}

impl JwtToken {
    /// Create a new JWT token from a string.
    /// Decodes the token and holds the decoded claims.
    /// Rejects the token if it is not valid or if the claims are not valid.
    /// 
    /// Doesn't validate the token though, that is done in the JwtService.
    pub fn new(token: String) -> Result<Self, jsonwebtoken::errors::Error> {
        let claims = Self::decode(&token)?;
        Ok(Self { token, decoded: claims })
    }

    fn decode(token: &str) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
        let mut validation = jsonwebtoken::Validation::default();
        validation.insecure_disable_signature_validation();
        validation.validate_aud = false;
        validation.validate_exp = false;
        let key = DecodingKey::from_secret(&[]); // Fake key. Needed because the method requires a key. It's not validated though.
        jsonwebtoken::decode::<Claims>(&token, &key, &validation)
    }
    
    /// Get the decoded claims of the JWT token.
    pub fn decoded(&self) -> &TokenData<Claims> {
        &self.decoded
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_jwt_token() {
        let token = JwtToken::new("eyJ0eXAiOiJKV1QiLCJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJudDRtbXFuZXB5OWlwYmV6M3Nmc3J0amtmcHNtZjZ5dXFhdW1xdTh0aWVqZ2pneXdhNXVvIiwiaXNzIjoiOHBpbnh4Z3FzNDFuNGFpZGlkZW53NWFwcXAxdXJmbXpkenRyOGp0NGFicmtkbjQzNWV3byIsImV4cCI6MTc0NjUyODA0MywiaWF0IjoxNzQ2NTI4MDMzLCJqdGkiOiJlM2E2ZmQxZi01M2I1LTQ4ODMtYjc2Yi01MDgzNzVhMjkzMTgiLCJjYXBhYmlsaXRpZXMiOltdfQ.SCnQqWnaKn08MUTE8YWclENMvJWt7TS1-4MnwY7KnqJ1Pmrxntqbx3xg77Cdh196CIbMviEPcSYsUr1dPP8_eg".to_string()).unwrap();
        assert_eq!(token.decoded().claims.sub.to_z32(), "nt4mmqnepy9ipbez3sfsrtjkfpsmf6yuqaumqu8tiejgjgywa5uo");
    }

    #[test]
    fn test_decode_jwt_token_fail() {
        let result = JwtToken::new("eQiLCJhbGciOiJFUzI1NiJ9.eyJzdWIiOiJudDRtbXFuZXB5OWlwYmV6M3Nmc3J0amtmcHNtZjZ5dXFhdW1xdTh0aWVqZ2pneXdhNXVvIiwiaXNzIjoiOHBpbnh4Z3FzNDFuNGFpZGlkZW53NWFwcXAxdXJmbXpkenRyOGp0NGFicmtkbjQzNWV3byIsImV4cCI6MTc0NjUyODA0MywiaWF0IjoxNzQ2NTI4MDMzLCJqdGkiOiJlM2E2ZmQxZi01M2I1LTQ4ODMtYjc2Yi01MDgzNzVhMjkzMTgiLCJjYXBhYmlsaXRpZXMiOltdfQ.SCnQqWnaKn08MUTE8YWclENMvJWt7TS1-4MnwY7KnqJ1Pmrxntqbx3xg77Cdh196CIbMviEPcSYsUr1dPP8_eg".to_string());
        assert!(result.is_err());
    }
}
