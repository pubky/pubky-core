use pubky_common::{capabilities::Capabilities, crypto::PublicKey};
use url::Url;

use super::{
    DeepLinkParseError,
    query_params::{
        append_signup_params, optional_query, parse_capabilities_or_default, parse_homeserver,
        parse_optional_relay, parse_optional_secret,
    },
    typed_deep_link::{DeepLinkIntent, DeepLinkParams, TypedDeepLink},
};

/// Intent marker for legacy signup deep links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignupIntent;

impl DeepLinkIntent for SignupIntent {
    const NAME: &'static str = "signup";
}

/// Typed parameters for signup deep links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignupParams {
    /// Capabilities requested by the app.
    pub capabilities: Capabilities,
    /// Base HTTP relay URL. `None` for a direct signup link.
    pub relay: Option<Url>,
    /// Secret used to derive the encrypted relay channel. `None` for a direct
    /// signup link.
    pub secret: Option<[u8; 32]>,
    /// Homeserver public key.
    pub homeserver: PublicKey,
    /// Optional signup token.
    pub signup_token: Option<String>,
}

impl SignupParams {
    /// Returns `true` when this is a direct signup link (no relay/secret).
    #[must_use]
    pub fn is_direct_signup(&self) -> bool {
        self.relay.is_none() && self.secret.is_none()
    }
}

impl DeepLinkParams for SignupParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        let relay = parse_optional_relay(url)?;
        let secret = parse_optional_secret(url)?;
        // `relay` and `secret` are only meaningful together: both present is a
        // relayed signup, both absent is a direct signup. One without the other
        // is malformed.
        if relay.is_some() != secret.is_some() {
            let missing = if relay.is_none() { "relay" } else { "secret" };
            return Err(DeepLinkParseError::MissingQueryParameter(missing));
        }
        Ok(Self {
            capabilities: parse_capabilities_or_default(url)?,
            relay,
            secret,
            homeserver: parse_homeserver(url)?,
            signup_token: optional_query(url, "st"),
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        append_signup_params(
            url,
            (!self.capabilities.is_empty()).then_some(&self.capabilities),
            self.relay.as_ref(),
            self.secret.as_ref(),
            &self.homeserver,
            self.signup_token.as_deref(),
        );
    }
}

/// A deep link for signing up to a Pubky homeserver via the legacy cookie flow.
pub type SignupDeepLink = TypedDeepLink<SignupIntent, SignupParams>;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::actors::auth::deep_links::DeepLinkScheme;

    const HOMESERVER: &str = "5jsjx1o6fzu6aeeo697r3i5rx15zq41kikcye8wtwdqm4nb4tryo";

    #[test]
    fn parses_signup_deep_link_without_signup_token() {
        let deep_link: SignupDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&hs={HOMESERVER}"
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.scheme(), DeepLinkScheme::PubkyAuth);
        assert_eq!(deep_link.intent(), "signup");
        assert_eq!(deep_link.params().homeserver.z32(), HOMESERVER);
        assert_eq!(deep_link.params().signup_token, None);
    }

    #[test]
    fn parses_signup_deep_link_with_signup_token() {
        let deep_link: SignupDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&hs={HOMESERVER}&st=1234567890"
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.params().signup_token, Some("1234567890".into()));
    }

    #[test]
    fn parses_signup_deep_link_with_extra_query_params() {
        let deep_link: SignupDeepLink = format!(
            "pubkyauth://signup?caps=/pub/pubky.app/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8&hs={HOMESERVER}&cid=franky.pubky.app&cpk=not-a-public-key"
        )
        .parse()
        .unwrap();

        assert_eq!(deep_link.intent(), "signup");
    }

    #[test]
    fn creates_signup_deep_link_from_params() {
        let capabilities = Capabilities::builder().read_write("/").finish();
        let relay = Url::parse("https://httprelay.pubky.app/inbox/").unwrap();
        let homeserver = PublicKey::from_str(HOMESERVER).unwrap();
        let deep_link = SignupDeepLink::new(
            DeepLinkScheme::PubkyAuth,
            SignupParams {
                capabilities,
                relay: Some(relay),
                secret: Some([123; 32]),
                homeserver,
                signup_token: Some("1234567890".into()),
            },
        );
        let parsed_again = SignupDeepLink::parse_url(&deep_link.to_url()).unwrap();

        assert_eq!(parsed_again, deep_link);
    }

    #[test]
    fn parses_direct_signup_deep_link() {
        let deep_link: SignupDeepLink = format!("pubkyauth://signup?hs={HOMESERVER}")
            .parse()
            .unwrap();

        assert_eq!(deep_link.scheme(), DeepLinkScheme::PubkyAuth);
        assert_eq!(deep_link.intent(), "signup");
        assert_eq!(deep_link.params().homeserver.z32(), HOMESERVER);
        assert_eq!(deep_link.params().relay, None);
        assert_eq!(deep_link.params().secret, None);
        assert!(deep_link.params().capabilities.is_empty());
        assert!(deep_link.params().is_direct_signup());
        assert_eq!(deep_link.params().signup_token, None);
    }

    #[test]
    fn parses_direct_signup_deep_link_with_token() {
        let deep_link: SignupDeepLink = format!("pubkyauth://signup?hs={HOMESERVER}&st=1234567890")
            .parse()
            .unwrap();

        assert!(deep_link.params().is_direct_signup());
        assert_eq!(deep_link.params().signup_token, Some("1234567890".into()));
    }

    #[test]
    fn direct_signup_deep_link_round_trips() {
        let homeserver = PublicKey::from_str(HOMESERVER).unwrap();
        let deep_link = SignupDeepLink::new(
            DeepLinkScheme::PubkyAuth,
            SignupParams {
                capabilities: Capabilities::default(),
                relay: None,
                secret: None,
                homeserver,
                signup_token: None,
            },
        );

        // A direct link serializes to only the homeserver — no caps/relay/secret.
        let serialized = deep_link.to_string();
        assert!(serialized.contains(&format!("hs={HOMESERVER}")));
        assert!(!serialized.contains("caps="));
        assert!(!serialized.contains("relay="));
        assert!(!serialized.contains("secret="));

        let parsed_again = SignupDeepLink::parse_url(&deep_link.to_url()).unwrap();
        assert_eq!(parsed_again, deep_link);
    }

    #[test]
    fn rejects_signup_relay_without_secret() {
        let error =
            format!("pubkyauth://signup?hs={HOMESERVER}&relay=https://httprelay.pubky.app/inbox/")
                .parse::<SignupDeepLink>()
                .unwrap_err();

        assert!(matches!(
            error,
            DeepLinkParseError::MissingQueryParameter("secret")
        ));
    }

    #[test]
    fn rejects_signup_secret_without_relay() {
        let error = format!(
            "pubkyauth://signup?hs={HOMESERVER}&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8"
        )
        .parse::<SignupDeepLink>()
        .unwrap_err();

        assert!(matches!(
            error,
            DeepLinkParseError::MissingQueryParameter("relay")
        ));
    }

    #[test]
    fn rejects_missing_homeserver() {
        let error = "pubkyauth://signup?caps=/:rw&relay=https://httprelay.pubky.app/inbox/&secret=kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8"
            .parse::<SignupDeepLink>()
            .unwrap_err();

        assert!(matches!(
            error,
            DeepLinkParseError::MissingQueryParameter("hs")
        ));
    }
}
