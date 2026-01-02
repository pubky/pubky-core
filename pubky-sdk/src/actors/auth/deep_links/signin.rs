use std::{fmt::Display, str::FromStr};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pubky_common::capabilities::Capabilities;
use url::Url;

use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for signing into a Pubky homeserver.
/// Supported formats:
/// - New format with intent: <pubkyauth://signin?caps={}&relay={}&secret>={}
/// - Old format without intent: <pubkyauth:///?caps={}&relay={}&secret>={}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigninDeepLink {
    capabilities: Capabilities,
    relay: Url,
    secret: [u8; 32],
}

impl SigninDeepLink {
    /// Create a new signin deep link.
    ///
    /// # Arguments
    /// * `capabilities` - The capabilities to use for the signin flow.
    /// * `relay` - The relay to use for the signin flow.
    /// * `secret` - The secret to use for the signin flow.
    #[must_use]
    pub fn new(capabilities: Capabilities, relay: Url, secret: [u8; 32]) -> Self {
        Self {
            capabilities,
            relay,
            secret,
        }
    }

    /// Get the capabilities for the signin flow.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Get the relay for the signin flow.
    #[must_use]
    pub fn relay(&self) -> &Url {
        &self.relay
    }

    /// Get the secret for the signin flow.
    #[must_use]
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl Display for SigninDeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pubkyauth://signin?caps={}&relay={}&secret={}",
            self.capabilities,
            self.relay,
            URL_SAFE_NO_PAD.encode(self.secret)
        )
    }
}

impl FromStr for SigninDeepLink {
    type Err = DeepLinkParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if !DEEP_LINK_SCHEMES.contains(&url.scheme()) {
            return Err(DeepLinkParseError::InvalidSchema("pubkyauth or pubkyring"));
        }
        let intent = url.host_str().unwrap_or("").to_string();
        if intent != "signin" {
            return Err(DeepLinkParseError::InvalidIntent("signin"));
        }

        let raw_caps = url
            .query_pairs()
            .find(|(key, _)| key == "caps")
            .ok_or(DeepLinkParseError::MissingQueryParameter("caps"))?
            .1
            .to_string();
        let capabilities: Capabilities = raw_caps
            .as_str()
            .try_into()
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("caps", Box::new(e)))?;

        let raw_relay = url
            .query_pairs()
            .find(|(key, _)| key == "relay")
            .ok_or(DeepLinkParseError::MissingQueryParameter("relay"))?
            .1
            .to_string();
        let relay = Url::parse(&raw_relay)
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("relay", Box::new(e)))?;

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

        Ok(SigninDeepLink {
            capabilities,
            relay,
            secret,
        })
    }
}

impl From<SigninDeepLink> for Url {
    fn from(val: SigninDeepLink) -> Self {
        Url::parse(&val.to_string()).expect("Should be able to parse the deep link")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signin_deep_link_parse() {
        let capabilities = Capabilities::builder()
            .read_write("/")
            .read("/test")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/link/").unwrap();
        let secret = [123; 32];
        let deep_link = SigninDeepLink::new(capabilities.clone(), relay.clone(), secret.clone());
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signin?caps={}&relay={}&secret={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(&secret)
            )
        );
        let deep_link_parsed = SigninDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }
}
