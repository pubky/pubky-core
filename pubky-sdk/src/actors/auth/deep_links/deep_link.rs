use std::{fmt::Display, str::FromStr};

use url::Url;

use crate::actors::auth::deep_links::{
    DEEP_LINK_SCHEMES, error::DeepLinkParseError, signin::SigninDeepLink, signup::SignupDeepLink,
};

/// A parsed Pubky deep link.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Doesn't really matter in this case as this enum is never stored in a large amount of data"
)]
pub enum DeepLink {
    /// A signin deep link.
    Signin(SigninDeepLink),
    /// A signup deep link.
    Signup(SignupDeepLink),
}

impl DeepLink {
    /// Convert the deep link to a simple URL.
    #[must_use]
    pub fn to_url(self) -> Url {
        match self {
            DeepLink::Signin(signin) => signin.clone().into(),
            DeepLink::Signup(signup) => signup.clone().into(),
        }
    }
}

impl Display for DeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeepLink::Signin(signin) => write!(f, "{signin}"),
            DeepLink::Signup(signup) => write!(f, "{signup}"),
        }
    }
}

impl FromStr for DeepLink {
    type Err = DeepLinkParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = Url::parse(s)?;
        if !DEEP_LINK_SCHEMES.contains(&url.scheme()) {
            return Err(DeepLinkParseError::InvalidSchema("pubkyauth or pubkyring"));
        }
        let intent = url.host_str().unwrap_or("").to_string();
        match intent.as_str() {
            "signin" => Ok(DeepLink::Signin(s.parse()?)),
            "signup" => Ok(DeepLink::Signup(s.parse()?)),
            "" => {
                // Backwards compatible with old format
                let mut url = url.clone();
                url.set_host(Some("signin"))?;
                let string_value = url.to_string();
                Ok(DeepLink::Signin(string_value.parse()?))
            }
            _ => Err(DeepLinkParseError::InvalidIntent("")),
        }
    }
}

impl From<DeepLink> for Url {
    fn from(val: DeepLink) -> Self {
        val.to_url()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_deep_link_signin() {
        let deep_link = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signin(_)));
    }

    #[test]
    fn test_parse_deep_link_signin_old_format() {
        let deep_link = "pubkyauth:///?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signin(_)));
    }

    #[test]
    fn test_parse_deep_link_signup() {
        let deep_link = "pubkyauth://signup?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/link&hs=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo&st=1234567890";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signup(_)));
    }
}
