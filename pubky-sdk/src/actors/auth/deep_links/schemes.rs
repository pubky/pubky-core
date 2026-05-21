#![allow(
    dead_code,
    reason = "Scheme type is introduced before the deep link parser refactor uses it"
)]

use std::{fmt::Display, str::FromStr};

use super::DeepLinkParseError;

const PUBKY_AUTH_SCHEME: &str = "pubkyauth";
/// Deprecated schema
const PUBKY_RING_SCHEME: &str = "pubkyring";
const EXPECTED_DEEP_LINK_SCHEMES: &str = "pubkyauth or pubkyring";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeepLinkScheme {
    PubkyAuth,
    PubkyRing,
}

impl DeepLinkScheme {
    /// Return the canonical URI scheme string.
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::PubkyAuth => PUBKY_AUTH_SCHEME,
            Self::PubkyRing => PUBKY_RING_SCHEME,
        }
    }
}

impl Display for DeepLinkScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for DeepLinkScheme {
    type Err = DeepLinkParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            PUBKY_AUTH_SCHEME => Ok(Self::PubkyAuth),
            PUBKY_RING_SCHEME => Ok(Self::PubkyRing),
            _ => Err(DeepLinkParseError::InvalidSchema(
                EXPECTED_DEEP_LINK_SCHEMES,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pubky_auth_scheme() {
        let scheme: DeepLinkScheme = "pubkyauth".parse().unwrap();

        assert_eq!(scheme, DeepLinkScheme::PubkyAuth);
    }

    #[test]
    fn parses_pubky_ring_scheme() {
        let scheme: DeepLinkScheme = "pubkyring".parse().unwrap();

        assert_eq!(scheme, DeepLinkScheme::PubkyRing);
    }

    #[test]
    fn rejects_unknown_scheme() {
        let error = "https".parse::<DeepLinkScheme>().unwrap_err();

        assert!(matches!(error, DeepLinkParseError::InvalidSchema(_)));
    }

    #[test]
    fn formats_pubky_auth_scheme() {
        assert_eq!(DeepLinkScheme::PubkyAuth.to_string(), "pubkyauth");
    }

    #[test]
    fn formats_pubky_ring_scheme() {
        assert_eq!(DeepLinkScheme::PubkyRing.to_string(), "pubkyring");
    }
}
