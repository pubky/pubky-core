use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pubky_common::capabilities::Capabilities;
use std::{
    collections::HashMap,
    fmt::{Display, Write},
    str::FromStr,
};
use url::Url;

use crate::PublicKey;
use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for signing up to a Pubky homeserver.
/// Supported formats:
/// - <pubkyauth://signup?caps={}&relay={}&secret={base64_encoded_secret}&hs={homeserver_public_key}&st={signup_token}>
/// - <pubkyauth://signup?hs={homeserver_public_key}&st={signup_token}>
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupDeepLink {
    capabilities: Capabilities,
    caps_in_url: bool,
    relay: Option<Url>,
    secret: Option<[u8; 32]>,
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
            caps_in_url: true,
            relay: Some(relay),
            secret: Some(secret),
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
    pub fn relay(&self) -> Option<&Url> {
        self.relay.as_ref()
    }

    /// Get the secret for the signup flow.
    #[must_use]
    pub fn secret(&self) -> Option<&[u8; 32]> {
        self.secret.as_ref()
    }

    /// Returns true if this deep link represents a direct signup flow.
    ///
    /// Direct signup links omit relay/secret and only include the homeserver
    /// and optional signup token.
    #[must_use]
    pub fn is_direct_signup(&self) -> bool {
        self.relay.is_none() && self.secret.is_none()
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
        // Canonical query ordering: hs, st, relay, secret, caps.
        let mut url = "pubkyauth://signup?".to_string();
        write!(&mut url, "hs={}", self.homeserver.z32())?;
        if let Some(signup_token) = self.signup_token.as_ref() {
            write!(&mut url, "&st={signup_token}")?;
        }
        if let Some(relay) = self.relay.as_ref() {
            write!(&mut url, "&relay={relay}")?;
        }
        if let Some(secret) = self.secret.as_ref() {
            write!(&mut url, "&secret={}", URL_SAFE_NO_PAD.encode(secret))?;
        }
        if self.caps_in_url {
            write!(&mut url, "&caps={}", self.capabilities)?;
        }
        write!(f, "{url}")?;
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
        let mut query_params: HashMap<String, String> = HashMap::new();
        for (key, value) in url.query_pairs() {
            query_params
                .entry(key.to_string())
                .or_insert_with(|| value.to_string());
        }
        let raw_caps = query_params.get("caps").cloned();
        let (capabilities, caps_in_url) = match raw_caps {
            Some(raw_caps) => (
                raw_caps
                    .as_str()
                    .try_into()
                    .map_err(|e| DeepLinkParseError::InvalidQueryParameter("caps", Box::new(e)))?,
                true,
            ),
            None => (Capabilities::default(), false),
        };

        let raw_relay = query_params.get("relay").cloned();
        let raw_secret = query_params.get("secret").cloned();

        let (relay, secret) = match (raw_relay, raw_secret) {
            (Some(raw_relay), Some(raw_secret)) => {
                let relay = Url::parse(&raw_relay)
                    .map_err(|e| DeepLinkParseError::InvalidQueryParameter("relay", Box::new(e)))?;
                let secret = URL_SAFE_NO_PAD.decode(raw_secret.as_str()).map_err(|e| {
                    DeepLinkParseError::InvalidQueryParameter("secret", Box::new(e))
                })?;
                let secret: [u8; 32] = secret.try_into().map_err(|e: Vec<u8>| {
                    let msg = format!("Expected 32 bytes, got {}", e.len());
                    DeepLinkParseError::InvalidQueryParameter(
                        "secret",
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
                    )
                })?;
                (Some(relay), Some(secret))
            }
            (None, None) => (None, None),
            (None, Some(_)) => {
                return Err(DeepLinkParseError::MissingQueryParameter("relay"));
            }
            (Some(_), None) => {
                return Err(DeepLinkParseError::MissingQueryParameter("secret"));
            }
        };

        let raw_homeserver = query_params
            .get("hs")
            .cloned()
            .ok_or(DeepLinkParseError::MissingQueryParameter("hs"))?;
        let homeserver = PublicKey::try_from_z32(raw_homeserver.as_str())
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("hs", Box::new(e)))?;

        let signup_token = query_params.get("st").cloned();

        Ok(SignupDeepLink {
            capabilities,
            caps_in_url,
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
            secret,
            homeserver.clone(),
            None,
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?hs={}&relay={}&secret={}&caps={}",
                homeserver.z32(),
                relay,
                URL_SAFE_NO_PAD.encode(secret),
                capabilities
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
            secret,
            homeserver.clone(),
            Some(signup_token.to_string()),
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?hs={}&st={}&relay={}&secret={}&caps={}",
                homeserver.z32(),
                signup_token,
                relay,
                URL_SAFE_NO_PAD.encode(secret),
                capabilities
            )
        );
        let deep_link_parsed = SignupDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }

    #[test]
    fn test_signup_deep_link_parse_direct() {
        let homeserver =
            PublicKey::from_str("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo").unwrap();
        let deep_link_str = format!("pubkyauth://signup?hs={}", homeserver.z32());
        let deep_link_parsed = SignupDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed.homeserver(), &homeserver);
        assert_eq!(deep_link_parsed.relay(), None);
        assert_eq!(deep_link_parsed.secret(), None);
    }
}
