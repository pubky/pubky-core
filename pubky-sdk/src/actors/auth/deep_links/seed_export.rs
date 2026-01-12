use std::{fmt::Display, str::FromStr};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use url::Url;

use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for exporting a user secret to a signer like Pubky Ring.
/// Supported formats:
/// - <pubkyauth://secret_export?secret=base64_encoded_secret>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedExportDeepLink {
    secret: [u8; 32],
}

impl SeedExportDeepLink {
    /// Create a new seed export deep link.
    ///
    /// # Arguments
    /// * `secret` - The keypair secret to export.
    #[must_use]
    pub fn new(secret: [u8; 32]) -> Self {
        Self { secret }
    }

    /// Get the secret for the seed export flow.
    #[must_use]
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Display for SeedExportDeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pubkyauth://secret_export?secret={}",
            URL_SAFE_NO_PAD.encode(self.secret)
        )
    }
}

impl FromStr for SeedExportDeepLink {
    type Err = DeepLinkParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if !DEEP_LINK_SCHEMES.contains(&url.scheme()) {
            return Err(DeepLinkParseError::InvalidSchema("pubkyauth or pubkyring"));
        }
        let intent = url.host_str().unwrap_or("").to_string();
        if intent != "secret_export" {
            return Err(DeepLinkParseError::InvalidIntent("secret_export"));
        }

        let raw_secret = url
            .query_pairs()
            .find(|(key, _)| key == "secret")
            .ok_or(DeepLinkParseError::MissingQueryParameter("secret"))?
            .1
            .to_string();
        let secret = URL_SAFE_NO_PAD
            .decode(raw_secret.as_str())
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("secret", Box::new(e)))?;
        let secret: [u8; 32] = secret.try_into().map_err(|e: Vec<u8>| {
            let msg = format!("Expected 32 bytes, got {}", e.len());
            DeepLinkParseError::InvalidQueryParameter(
                "secret",
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
            )
        })?;

        Ok(SeedExportDeepLink { secret })
    }
}

impl From<SeedExportDeepLink> for Url {
    fn from(val: SeedExportDeepLink) -> Self {
        Url::parse(&val.to_string()).expect("Should be able to parse the deep link")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Keypair;

    #[test]
    fn test_signin_deep_link_parse() {
        let keypair = Keypair::random();
        let secret = keypair.seed();
        let deep_link = SeedExportDeepLink::new(secret);
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://secret_export?secret={}",
                URL_SAFE_NO_PAD.encode(secret)
            )
        );
        let deep_link_parsed = SeedExportDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }
}
