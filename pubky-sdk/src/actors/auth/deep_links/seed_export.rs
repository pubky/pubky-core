use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use url::Url;

use super::{
    DeepLinkParseError,
    typed_deep_link::{DeepLinkIntent, DeepLinkParams, TypedDeepLink, parse_secret},
};

/// Intent marker for seed-export deep links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretExportIntent;

impl DeepLinkIntent for SecretExportIntent {
    const NAME: &'static str = "secret_export";
}

/// A typed seed-export deep link.
pub type SeedExportDeepLink = TypedDeepLink<SecretExportIntent, SeedExportParams>;

/// Typed parameters for a seed-export deep link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedExportParams {
    /// The keypair secret to export.
    pub secret: [u8; 32],
}

impl DeepLinkParams for SeedExportParams {
    fn parse(url: &Url) -> Result<Self, DeepLinkParseError> {
        Ok(Self {
            secret: parse_secret(url)?,
        })
    }

    fn append_query_pairs(&self, url: &mut Url) {
        url.query_pairs_mut()
            .append_pair("secret", &URL_SAFE_NO_PAD.encode(self.secret));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::auth::deep_links::DeepLinkScheme;

    const SECRET: &str = "kqnceEMgrNQM_xi06oQXjA3cJHX_RQmw1BY6JE1bse8";
    const SECRET_BYTES: [u8; 32] = [
        146, 169, 220, 120, 67, 32, 172, 212, 12, 255, 24, 180, 234, 132, 23, 140, 13, 220, 36,
        117, 255, 69, 9, 176, 212, 22, 58, 36, 77, 91, 177, 239,
    ];

    #[test]
    fn parses_seed_export_deep_link() {
        let deep_link: SeedExportDeepLink =
            format!("pubkyring://secret_export?secret={SECRET}")
                .parse()
                .unwrap();

        assert_eq!(deep_link.scheme(), DeepLinkScheme::PubkyRing);
        assert_eq!(deep_link.intent(), "secret_export");
        assert_eq!(deep_link.params().secret, SECRET_BYTES);
    }

    #[test]
    fn converts_seed_export_deep_link_to_url() {
        let deep_link: SeedExportDeepLink =
            format!("pubkyring://secret_export?secret={SECRET}")
                .parse()
                .unwrap();
        let url = deep_link.to_url();

        assert_eq!(url.scheme(), "pubkyring");
        assert_eq!(url.host_str(), Some("secret_export"));
        assert_eq!(SeedExportDeepLink::parse_url(&url).unwrap(), deep_link);
    }

    #[test]
    fn creates_seed_export_deep_link_from_params() {
        let deep_link = SeedExportDeepLink::new(
            DeepLinkScheme::PubkyAuth,
            SeedExportParams {
                secret: SECRET_BYTES,
            },
        );
        let url = deep_link.to_url();
        let parsed_again = SeedExportDeepLink::parse_url(&url).unwrap();

        assert_eq!(deep_link.scheme(), DeepLinkScheme::PubkyAuth);
        assert_eq!(deep_link.intent(), "secret_export");
        assert_eq!(deep_link.params().secret, SECRET_BYTES);
        assert_eq!(parsed_again, deep_link);
    }

    #[test]
    fn display_formats_as_url() {
        let deep_link: SeedExportDeepLink =
            format!("pubkyauth://secret_export?secret={SECRET}")
                .parse()
                .unwrap();

        assert_eq!(deep_link.to_string(), deep_link.to_url().to_string());
    }

    #[test]
    fn converts_owned_typed_deep_link_into_url() {
        let deep_link: SeedExportDeepLink =
            format!("pubkyauth://secret_export?secret={SECRET}")
                .parse()
                .unwrap();
        let url: Url = deep_link.into();

        assert_eq!(url.scheme(), "pubkyauth");
        assert_eq!(url.host_str(), Some("secret_export"));
        url.query_pairs()
            .find(|(key, _)| key == "secret")
            .map(|(_, value)| assert_eq!(value, SECRET))
            .expect("Expected 'secret' query parameter");
    }
}
