use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pkarr::PublicKey;
use pubky_common::capabilities::Capabilities;
use std::{fmt::Display, str::FromStr};
use url::Url;

use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for signing up to a Pubky homeserver.
/// Supported formats:
/// - New format with intent: <pubkyauth://signup?caps={}&relay={}&secret>={}
/// - Old format without intent: <pubkyauth://?caps={}&relay={}&secret>={}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupDeepLink {
    capabilities: Capabilities,
    relay: Url,
    secret: [u8; 32],
    homeserver: PublicKey,
    signup_token: Option<String>,
}

impl SignupDeepLink {
    /// Create a new signup deep link.
    ///
    /// # Arguments
    /// * `capabilities` - The capabilities to use for the signup flow.
    /// * `relay` - The relay to use for the signup flow.
    /// * `secret` - The secret to use for the signup flow.
    /// * `homeserver` - The homeserver to use for the signup flow.
    /// * `signup_token` - The signup token to use for the signup flow.
    #[must_use]
    pub fn new(
        capabilities: Capabilities,
        relay: Url,
        secret: [u8; 32],
        homeserver: PublicKey,
        signup_token: Option<String>,
    ) -> Self {
        Self {
            capabilities,
            relay,
            secret,
            homeserver,
            signup_token,
        }
    }

    /// Get the capabilities for the signup flow.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Get the relay for the signup flow.
    #[must_use]
    pub fn relay(&self) -> &Url {
        &self.relay
    }

    /// Get the secret for the signup flow.
    #[must_use]
    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    /// Get the homeserver for the signup flow.
    #[must_use]
    pub fn homeserver(&self) -> &PublicKey {
        &self.homeserver
    }

    /// Get the signup token for the signup flow.
    #[must_use]
    pub fn signup_token(&self) -> Option<String> {
        self.signup_token.clone()
    }
}

impl Display for SignupDeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let url = format!(
            "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}",
            self.capabilities,
            self.relay,
            URL_SAFE_NO_PAD.encode(self.secret),
            self.homeserver
        );
        write!(f, "{url}")?;
        if let Some(signup_token) = self.signup_token.as_ref() {
            write!(f, "&st={signup_token}")?;
        }
        Ok(())
    }
}

impl FromStr for SignupDeepLink {
    type Err = DeepLinkParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if !DEEP_LINK_SCHEMES.contains(&url.scheme()) {
            return Err(DeepLinkParseError::InvalidSchema("pubkyauth or pubkyring"));
        }
        let intent = url.host_str().unwrap_or("").to_string();
        if intent != "signup" {
            return Err(DeepLinkParseError::InvalidIntent("signup"));
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

        let raw_homeserver = url
            .query_pairs()
            .find(|(key, _)| key == "hs")
            .ok_or(DeepLinkParseError::MissingQueryParameter("hs"))?
            .1
            .to_string();
        let homeserver = PublicKey::try_from(raw_homeserver.as_str())
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("hs", Box::new(e)))?;

        let signup_token = url
            .query_pairs()
            .find(|(key, _)| key == "st")
            .map(|(_, value)| value.to_string());

        Ok(SignupDeepLink {
            capabilities,
            relay,
            secret,
            homeserver,
            signup_token,
        })
    }
}

impl From<SignupDeepLink> for Url {
    fn from(val: SignupDeepLink) -> Self {
        Url::parse(&val.to_string()).expect("Should be able to parse the deep link")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signup_deep_link_parse_no_signup_token() {
        let capabilities = Capabilities::builder()
            .read_write("/")
            .read("/test")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/link/").unwrap();
        let secret = [123; 32];
        let homeserver =
            PublicKey::from_str("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo").unwrap();
        let deep_link = SignupDeepLink::new(
            capabilities.clone(),
            relay.clone(),
            secret.clone(),
            homeserver.clone(),
            None,
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(&secret),
                homeserver
            )
        );
        let deep_link_parsed = SignupDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }

    #[test]
    fn test_signup_deep_link_parse_with_signup_token() {
        let capabilities = Capabilities::builder()
            .read_write("/")
            .read("/test")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/link/").unwrap();
        let secret = [123; 32];
        let homeserver =
            PublicKey::from_str("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo").unwrap();
        let signup_token = "1234567890";
        let deep_link = SignupDeepLink::new(
            capabilities.clone(),
            relay.clone(),
            secret.clone(),
            homeserver.clone(),
            Some(signup_token.to_string()),
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}&st={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(&secret),
                homeserver,
                signup_token
            )
        );
        let deep_link_parsed = SignupDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }
}
