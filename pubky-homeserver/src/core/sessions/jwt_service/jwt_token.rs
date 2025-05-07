use std::ops::Deref;

use jsonwebtoken::{DecodingKey, TokenData};

use super::Claims;

/// A simple wrapper around a JWT token that also holds the decoded claims.
///
/// This is useful to avoid decoding the token multiple times.
///
/// Only valid for our own JWT tokens.
pub struct JwtToken {
    token: String,
    decoded: TokenData<Claims>,
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
        Ok(Self {
            token,
            decoded: claims,
        })
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

    pub fn raw(&self) -> &str {
        &self.token
    }
}
