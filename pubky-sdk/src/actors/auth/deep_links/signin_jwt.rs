use std::{fmt::Display, str::FromStr};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pubky_common::{auth::jws::ClientId, capabilities::Capabilities, crypto::PublicKey};
use url::Url;

use crate::actors::auth::deep_links::{DEEP_LINK_SCHEMES, error::DeepLinkParseError};

/// A deep link for signing in to a Pubky homeserver via the **grant + JWT**
/// (Proof-of-Possession) flow.
///
/// Format:
/// `pubkyauth://signin?caps=…&relay=…&secret=…&cid=…&cpk=…`
///
/// `cid` is the application identifier and `cpk` is the client public key
/// bound by the grant's `cnf` claim. Both are required — for the legacy
/// cookie flow without grant binding, use [`SigninDeepLink`](super::SigninDeepLink).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigninJwtDeepLink {
    capabilities: Capabilities,
    relay: Url,
    secret: [u8; 32],
    client_id: ClientId,
    client_pk: PublicKey,
}

impl SigninJwtDeepLink {
    /// Create a new JWT-mode signin deep link.
    #[must_use]
    pub fn new(
        capabilities: Capabilities,
        relay: Url,
        secret: [u8; 32],
        client_id: ClientId,
        client_pk: PublicKey,
    ) -> Self {
        Self {
            capabilities,
            relay,
            secret,
            client_id,
            client_pk,
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

impl Display for SigninJwtDeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "pubkyauth://signin?caps={}&relay={}&secret={}&cid={}&cpk={}",
            self.capabilities,
            self.relay,
            URL_SAFE_NO_PAD.encode(self.secret),
            self.client_id,
            self.client_pk.z32()
        )
    }
}

impl FromStr for SigninJwtDeepLink {
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

        Ok(SigninJwtDeepLink {
            capabilities,
            relay,
            secret,
            client_id,
            client_pk,
        })
    }
}

impl From<SigninJwtDeepLink> for Url {
    fn from(val: SigninJwtDeepLink) -> Self {
        Url::parse(&val.to_string()).expect("Should be able to parse the deep link")
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use super::*;

    #[test]
    fn test_signin_jwt_deep_link_parse() {
        let capabilities = Capabilities::builder()
            .read_write("/pub/franky.app/")
            .read("/pub/foo.bar/file")
            .finish();
        let relay = Url::parse("https://httprelay.pubky.app/inbox/").unwrap();
        let secret = [42; 32];
        let client_id = ClientId::new("franky.pubky.app").unwrap();
        let client_kp = Keypair::random();
        let client_pk = client_kp.public_key();

        let deep_link = SigninJwtDeepLink::new(
            capabilities.clone(),
            relay.clone(),
            secret,
            client_id.clone(),
            client_pk.clone(),
        );
        let deep_link_str = deep_link.to_string();
        assert_eq!(
            deep_link_str,
            format!(
                "pubkyauth://signin?caps={}&relay={}&secret={}&cid={}&cpk={}",
                capabilities,
                relay,
                URL_SAFE_NO_PAD.encode(secret),
                client_id,
                client_pk.z32()
            )
        );
        let deep_link_parsed = SigninJwtDeepLink::from_str(&deep_link_str).unwrap();
        assert_eq!(deep_link_parsed, deep_link);
    }

    #[test]
    fn test_signin_jwt_deep_link_rejects_missing_cpk() {
        // A signin URL with cid but no cpk must not parse as a JWT deep link.
        let url = "pubkyauth://signin?caps=/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&cid=franky.pubky.app";
        let err = SigninJwtDeepLink::from_str(url).unwrap_err();
        assert!(matches!(
            err,
            DeepLinkParseError::MissingQueryParameter("cpk")
        ));
    }

    #[test]
    fn test_signin_jwt_deep_link_rejects_missing_cid() {
        // A signin URL with cpk but no cid must not parse as a JWT deep link.
        let kp = Keypair::random();
        let url = format!(
            "pubkyauth://signin?caps=/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&cpk={}",
            kp.public_key().z32()
        );
        let err = SigninJwtDeepLink::from_str(&url).unwrap_err();
        assert!(matches!(
            err,
            DeepLinkParseError::MissingQueryParameter("cid")
        ));
    }
}
