use crate::errors::PubkyError;
use pkarr::PublicKey;
use url::Url;

pub trait IntoPubkyUrl {
    /// Check if this is a pubky:// URL
    fn is_pubky_uri(&self) -> bool;

    fn is_icann_url(&self) -> bool;

    /// Check if this is an HTTPS URL with a Pkarr domain (public key as host)
    fn is_pkarr_domain(&self) -> bool;

    /// Extract the public key from a Pubky URL (works for both pubky:// and Pkarr domain URLs)
    fn extract_public_key(&self) -> Result<PublicKey, PubkyError>;

    /// Convert to homeserver URL format (https://_pubky.<public_key>)
    fn to_pkarr_url(&self) -> Result<Url, PubkyError>;
}

impl IntoPubkyUrl for Url {
    fn is_pubky_uri(&self) -> bool {
        let s = self.domain().unwrap_or("").to_string();
        self.scheme() == "pubky" && PublicKey::try_from(s).is_ok()
    }

    fn is_icann_url(&self) -> bool {
        !self.is_pkarr_domain()
    }

    fn is_pkarr_domain(&self) -> bool {
        let domain = self.domain().unwrap_or("");
        PublicKey::try_from(domain).is_ok()
    }

    fn extract_public_key(&self) -> Result<PublicKey, PubkyError> {
        if self.is_pubky_uri() {
            let host = self.host_str().ok_or(PubkyError::MissingHost)?;
            PublicKey::try_from(host).map_err(|_| PubkyError::InvalidPublicKey(host.to_string()))
        } else if self.is_pkarr_domain() {
            let domain = self.domain().ok_or(PubkyError::MissingHost)?;
            PublicKey::try_from(domain)
                .map_err(|_| PubkyError::InvalidPublicKey(domain.to_string()))
        } else {
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }

    fn to_pkarr_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_uri() {
            let s = self.as_str();
            let normal = format!("https://_pubky.{}", s.split_at(8).1);
            Url::parse(normal.as_str()).map_err(PubkyError::UrlParseError)
        } else {
            Ok(self.to_owned())
        }
    }
}

impl IntoPubkyUrl for &str {
    fn is_pubky_uri(&self) -> bool {
        (self.starts_with("pubky://") || self.contains("_pubky"))
            && PublicKey::try_from(self.to_owned()).is_ok()
    }

    fn is_icann_url(&self) -> bool {
        (self.starts_with("https://") || self.starts_with("http://"))
            && PublicKey::try_from(self.to_owned()).is_err()
    }

    fn is_pkarr_domain(&self) -> bool {
        if let Ok(url) = Url::parse(self) {
            if url.scheme() != "https" {
                return false;
            };
            if let Some(domain) = url.domain() {
                return PublicKey::try_from(domain).is_ok();
            }
        }
        false
    }

    fn extract_public_key(&self) -> Result<PublicKey, PubkyError> {
        if let Ok(url) = Url::parse(self) {
            if url.is_pubky_uri() {
                // Extract public key from pubky://public_key/path format
                let after_protocol = &self[8..]; // Remove "pubky://"
                let public_key_part = after_protocol.split('/').next().ok_or_else(|| {
                    PubkyError::InvalidFormat("Missing public key in pubky URL".to_string())
                })?;

                PublicKey::try_from(public_key_part)
                    .map_err(|_| PubkyError::InvalidPublicKey(public_key_part.to_string()))
            } else if url.is_pkarr_domain() {
                let host = url.host_str().ok_or(PubkyError::MissingHost)?;
                // Remove _pubky. prefix if present
                let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);
                PublicKey::try_from(clean_host)
                    .map_err(|_| PubkyError::InvalidPublicKey(clean_host.to_string()))
            } else {
                Err(PubkyError::NotIntoPubkyUrl)
            }
        } else {
            Err(PubkyError::InvalidPublicKey(self.to_string()))
        }
    }

    fn to_pkarr_url(&self) -> Result<Url, PubkyError> {
        Url::parse(self)
            .map_err(PubkyError::UrlParseError)?
            .to_pkarr_url()
    }
}

#[cfg(not(wasm_browser))]
#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_is_pubky_uri() {
        let valid_pubky_uris = vec![
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo",
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json",
        ];
        for url in valid_pubky_uris {
            assert!(url.is_pubky_uri(), "Expected pubky URL: {}", url);
            let parsed = Url::parse(url).unwrap();
            assert!(parsed.is_pubky_uri(), "Expected pubky URL (Url): {}", url);
        }

        let invalid_pubky_uris = vec![
            "pubky:///pub/pubky.app/profile.json",
            "pubky://notarealkey/pub/pubky.app/profile.json",
        ];
        for url in invalid_pubky_uris {
            assert!(!url.is_pubky_uri(), "Expected not pubky URL: {}", url);
        }
    }

    #[test]
    fn test_extract_public_key() {
        let pk =
            PublicKey::try_from("operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo").unwrap();

        let url = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/";
        let key = url.extract_public_key();
        assert_eq!(key.unwrap(), pk);

        let url = "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
        let key = url.extract_public_key();
        assert_eq!(key.unwrap(), pk);

        let url = "https://example.com";
        let key = url.extract_public_key();
        assert!(matches!(key, Err(PubkyError::NotIntoPubkyUrl)));
    }

    #[test]
    fn test_is_pkarr_domain() {
        let valid_domains = vec![
            "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo",
            "https://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo",
        ];
        for url in valid_domains {
            assert!(url.is_pkarr_domain(), "Expected pkarr domain: {}", url);
            let parsed = Url::parse(url).unwrap();
            assert!(
                parsed.is_pkarr_domain(),
                "Expected pkarr domain (Url): {}",
                url
            );
        }

        let invalid_domains = vec!["https://example.com", "https://_pubky.notarealkey"];
        for url in invalid_domains {
            assert!(!url.is_pkarr_domain(), "Expected not pkarr domain: {}", url);
        }
    }

    #[test]
    fn test_pkarr_conversion() {
        let original = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let expected = "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        assert_eq!(expected, original.to_pkarr_url().unwrap().as_str());

        let pkarr_url = "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/";
        let result = pkarr_url.to_pkarr_url();
        assert_eq!(pkarr_url, result.unwrap().as_str());
    }
}
