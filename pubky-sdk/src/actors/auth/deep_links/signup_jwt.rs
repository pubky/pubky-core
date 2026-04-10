use std::{fmt::Display, str::FromStr};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pubky_common::{auth::jws::ClientId, capabilities::Capabilities, crypto::PublicKey};
use url::Url;

use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for signing up to a Pubky homeserver via the **grant + JWT**
/// (Proof-of-Possession) flow.
///
/// Format:
/// `pubkyauth://signup?caps=…&relay=…&secret=…&hs=…&st=…&cid=…&cpk=…`
///
/// `cid` is the application identifier and `cpk` is the client public key
/// bound by the grant's `cnf` claim. Both are required — for the legacy
/// cookie flow without grant binding, use [`SignupDeepLink`](super::SignupDeepLink).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupJwtDeepLink {
    capabilities: Capabilities,
    relay: Url,
    secret: [u8; 32],
    homeserver: PublicKey,
    signup_token: Option<String>,
    client_id: ClientId,
    client_pk: PublicKey,
}

impl SignupJwtDeepLink {
    /// Create a new JWT-mode signup deep link.
    #[must_use]
    #[allow(
        clippy::too_many_arguments,
        reason = "All fields are part of the wire format and equally required to construct a deep link"
    )]
    pub fn new(
        capabilities: Capabilities,
        relay: Url,
        secret: [u8; 32],
        homeserver: PublicKey,
        signup_token: Option<String>,
        client_id: ClientId,
        client_pk: PublicKey,
    ) -> Self {
        Self {
            capabilities,
            relay,
            secret,
            homeserver,
            signup_token,
            client_id,
            client_pk,
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

    /// Application identifier carried by this deep link.
    #[must_use]
    pub fn client_id(&self) -> &ClientId {
        &self.client_id
    }

    /// Client public key (`cnf` for the grant) carried by this deep link.
    #[must_use]
    pub fn client_pk(&self) -> &PublicKey {
        &self.client_pk
    }
}

impl Display for SignupJwtDeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}",
            self.capabilities,
            self.relay,
            URL_SAFE_NO_PAD.encode(self.secret),
            self.homeserver.z32()
        )?;
        if let Some(signup_token) = self.signup_token.as_ref() {
            write!(f, "&st={signup_token}")?;
        }
        write!(f, "&cid={}&cpk={}", self.client_id, self.client_pk.z32())
    }
}

impl FromStr for SignupJwtDeepLink {
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
        let homeserver = PublicKey::try_from_z32(raw_homeserver.as_str())
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("hs", Box::new(e)))?;

        let signup_token = url
            .query_pairs()
            .find(|(key, _)| key == "st")
            .map(|(_, value)| value.to_string());

        let raw_cid = url
            .query_pairs()
            .find(|(key, _)| key == "cid")
            .ok_or(DeepLinkParseError::MissingQueryParameter("cid"))?
            .1
            .to_string();
        let client_id = ClientId::new(&raw_cid)
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("cid", Box::new(e)))?;

        let raw_cpk = url
            .query_pairs()
            .find(|(key, _)| key == "cpk")
            .ok_or(DeepLinkParseError::MissingQueryParameter("cpk"))?
            .1
            .to_string();
        let client_pk = PublicKey::try_from_z32(&raw_cpk)
            .map_err(|e| DeepLinkParseError::InvalidQueryParameter("cpk", Box::new(e)))?;

        Ok(SignupJwtDeepLink {
            capabilities,
            relay,
            secret,
            homeserver,
            signup_token,
            client_id,
            client_pk,
        })
    }
}

impl From<SignupJwtDeepLink> for Url {
    fn from(val: SignupJwtDeepLink) -> Self {
        Url::parse(&val.to_string()).expect("Should be able to parse the deep link")
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use super::*;

    #[test]
    fn test_signup_jwt_deep_link_parse_no_signup_token() {
        let capabilities = Capabilities::builder()
            .read_write("/pub/franky.app/")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/inbox/").unwrap();
        let secret = [42; 32];
        let homeserver =
            PublicKey::from_str("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo").unwrap();
        let client_id = ClientId::new("franky.pubky.app").unwrap();
        let client_kp = Keypair::random();
        let client_pk = client_kp.public_key();

        let deep_link = SignupJwtDeepLink::new(
            capabilities.clone(),
            relay.clone(),
            secret,
            homeserver.clone(),
            None,
            client_id.clone(),
            client_pk.clone(),
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}&cid={}&cpk={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(secret),
                homeserver.z32(),
                client_id,
                client_pk.z32()
            )
        );
        let deep_link_parsed = SignupJwtDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }

    #[test]
    fn test_signup_jwt_deep_link_parse_with_signup_token() {
        let capabilities = Capabilities::builder()
            .read_write("/pub/franky.app/")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/inbox/").unwrap();
        let secret = [42; 32];
        let homeserver =
            PublicKey::from_str("5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo").unwrap();
        let signup_token = "1234567890";
        let client_id = ClientId::new("franky.pubky.app").unwrap();
        let client_kp = Keypair::random();
        let client_pk = client_kp.public_key();

        let deep_link = SignupJwtDeepLink::new(
            capabilities.clone(),
            relay.clone(),
            secret,
            homeserver.clone(),
            Some(signup_token.to_string()),
            client_id.clone(),
            client_pk.clone(),
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signup?caps={}&relay={}&secret={}&hs={}&st={}&cid={}&cpk={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(secret),
                homeserver.z32(),
                signup_token,
                client_id,
                client_pk.z32()
            )
        );
        let deep_link_parsed = SignupJwtDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }
}
