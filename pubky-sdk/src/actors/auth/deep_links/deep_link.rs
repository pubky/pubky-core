use std::{fmt::Display, str::FromStr};

use url::Url;

use crate::{
    actors::auth::deep_links::{
        DEEP_LINK_SCHEMES, error::DeepLinkParseError, signin::SigninDeepLink,
        signin_jwt::SigninJwtDeepLink, signup::SignupDeepLink, signup_jwt::SignupJwtDeepLink,
    },
    deep_links::seed_export::SeedExportDeepLink,
};

/// A parsed Pubky deep link.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    clippy::large_enum_variant,
    reason = "Doesn't really matter in this case as this enum is never stored in a large amount of data"
)]
pub enum DeepLink {
    /// A signin deep link (legacy cookie flow).
    Signin(SigninDeepLink),
    /// A signup deep link (legacy cookie flow).
    Signup(SignupDeepLink),
    /// A signin deep link for the grant + JWT flow.
    SigninJwt(SigninJwtDeepLink),
    /// A signup deep link for the grant + JWT flow.
    SignupJwt(SignupJwtDeepLink),
    /// A seed export deep link.
    SeedExport(SeedExportDeepLink),
}

impl DeepLink {
    /// Convert the deep link to a simple URL.
    #[must_use]
    pub fn to_url(self) -> Url {
        match self {
            DeepLink::Signin(signin) => signin.into(),
            DeepLink::Signup(signup) => signup.into(),
            DeepLink::SigninJwt(signin) => signin.into(),
            DeepLink::SignupJwt(signup) => signup.into(),
            DeepLink::SeedExport(seed_export) => seed_export.into(),
        }
    }
}

impl Display for DeepLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeepLink::Signin(signin) => write!(f, "{signin}"),
            DeepLink::Signup(signup) => write!(f, "{signup}"),
            DeepLink::SigninJwt(signin) => write!(f, "{signin}"),
            DeepLink::SignupJwt(signup) => write!(f, "{signup}"),
            DeepLink::SeedExport(seed_export) => write!(f, "{seed_export}"),
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
            "signin" => {
                if has_grant_params(&url)? {
                    Ok(DeepLink::SigninJwt(s.parse()?))
                } else {
                    Ok(DeepLink::Signin(s.parse()?))
                }
            }
            "signup" => {
                if has_grant_params(&url)? {
                    Ok(DeepLink::SignupJwt(s.parse()?))
                } else {
                    Ok(DeepLink::Signup(s.parse()?))
                }
            }
            "secret_export" => Ok(DeepLink::SeedExport(s.parse()?)),
            "" => {
                // Backwards compatible with old signin format (no host).
                let mut url = url.clone();
                url.set_host(Some("signin"))?;
                let string_value = url.to_string();
                string_value.parse()
            }
            _ => Err(DeepLinkParseError::InvalidIntent("")),
        }
    }
}

/// Returns `true` if both `cid` and `cpk` are present, `false` if neither is,
/// and an error if exactly one is present (partial grant binding is rejected).
fn has_grant_params(url: &Url) -> Result<bool, DeepLinkParseError> {
    let mut has_cid = false;
    let mut has_cpk = false;
    for (key, _) in url.query_pairs() {
        match key.as_ref() {
            "cid" => has_cid = true,
            "cpk" => has_cpk = true,
            _ => {}
        }
    }
    match (has_cid, has_cpk) {
        (true, true) => Ok(true),
        (false, false) => Ok(false),
        (true, false) => Err(DeepLinkParseError::MissingQueryParameter("cpk")),
        (false, true) => Err(DeepLinkParseError::MissingQueryParameter("cid")),
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
        let deep_link = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signin(_)));
    }

    #[test]
    fn test_parse_deep_link_signin_old_format() {
        let deep_link = "pubkyauth:///?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signin(_)));
    }

    #[test]
    fn test_parse_deep_link_signin_jwt() {
        let deep_link = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox&cid=franky.pubky.app&cpk=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::SigninJwt(_)));
    }

    #[test]
    fn test_parse_deep_link_signup() {
        let deep_link = "pubkyauth://signup?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox&hs=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo&st=1234567890";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::Signup(_)));
    }

    #[test]
    fn test_parse_deep_link_signup_jwt() {
        let deep_link = "pubkyauth://signup?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox&hs=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo&st=1234567890&cid=franky.pubky.app&cpk=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::SignupJwt(_)));
    }

    #[test]
    fn test_parse_deep_link_signin_partial_cid_rejected() {
        let deep_link = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox&cid=franky.pubky.app";
        let result: Result<DeepLink, _> = deep_link.parse();
        assert!(matches!(
            result,
            Err(DeepLinkParseError::MissingQueryParameter("cpk"))
        ));
    }

    #[test]
    fn test_parse_deep_link_signin_partial_cpk_rejected() {
        let deep_link = "pubkyauth://signin?caps=/pub/pubky.app/:rw&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&relay=https://httprelay.pubky.app/inbox&cpk=5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";
        let result: Result<DeepLink, _> = deep_link.parse();
        assert!(matches!(
            result,
            Err(DeepLinkParseError::MissingQueryParameter("cid"))
        ));
    }

    #[test]
    fn test_parse_deep_link_seed_export() {
        let deep_link =
            "pubkyauth://secret_export?secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8";
        let parsed: DeepLink = deep_link.parse().unwrap();
        assert!(matches!(parsed, DeepLink::SeedExport(_)));
    }
}
