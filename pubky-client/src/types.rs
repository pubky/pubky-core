use crate::errors::PubkyError;
use pkarr::PublicKey;
use reqwest::Url;

pub trait PubkyUrl {
    /// Check if this is a pubky:// URL
    fn is_pubky_url(&self) -> bool;

    fn is_icann_url(&self) -> bool;

    /// Check if this is an HTTPS URL with a Pkarr domain (public key as host)
    fn is_pkarr_domain(&self) -> bool;

    /// Check if this is any kind of Pubky-related URL (pubky:// or Pkarr domain)
    fn is_pubky_related(&self) -> bool {
        self.is_pubky_url() || self.is_pkarr_domain()
    }

    /// Extract the public key from a Pubky URL (works for both pubky:// and Pkarr domain URLs)
    fn extract_public_key(&self) -> Result<PublicKey, PubkyError>;

    /// Convert to homeserver URL format (https://_pubky.<public_key>)
    fn to_homeserver_url(&self) -> Result<Url, PubkyError>;

    /// Get the path component after the public key
    fn get_path(&self) -> Result<String, PubkyError>;

    /// Convert pubky:// URL to https:// equivalent
    fn to_https_url(&self) -> Result<Url, PubkyError>;
}

impl PubkyUrl for &str {
    fn is_pubky_url(&self) -> bool {
        self.starts_with("pubky://")
    }

    fn is_icann_url(&self) -> bool {
        self.starts_with("https://") && PublicKey::try_from(self.to_owned()).is_err()
    }

    fn is_pkarr_domain(&self) -> bool {
        if let Ok(url) = Url::parse(self) {
            if url.scheme() == "https" {
                if let Some(host) = url.host_str() {
                    // Remove _pubky. prefix if present for validation
                    let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);
                    return PublicKey::try_from(clean_host).is_ok();
                }
            }
        }
        false
    }

    fn extract_public_key(&self) -> Result<PublicKey, PubkyError> {
        if self.is_pubky_url() {
            // Extract public key from pubky://public_key/path format
            let after_protocol = &self[8..]; // Remove "pubky://"
            let public_key_part = after_protocol.split('/').next().ok_or_else(|| {
                PubkyError::InvalidFormat("Missing public key in pubky URL".to_string())
            })?;

            PublicKey::try_from(public_key_part)
                .map_err(|_| PubkyError::InvalidPublicKey(public_key_part.to_string()))
        } else if self.is_pkarr_domain() {
            let url = Url::parse(self)?;
            let host = url.host_str().ok_or(PubkyError::MissingHost)?;

            // Remove _pubky. prefix if present
            let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);

            PublicKey::try_from(clean_host)
                .map_err(|_| PubkyError::InvalidPublicKey(clean_host.to_string()))
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }

    fn to_homeserver_url(&self) -> Result<Url, PubkyError> {
        let public_key = self.extract_public_key()?;
        let path = self.get_path()?;

        let homeserver_url = if path.is_empty() || path == "/" {
            format!("https://_pubky.{}/", public_key)
        } else {
            format!("https://_pubky.{}{}", public_key, path)
        };

        Url::parse(&homeserver_url).map_err(PubkyError::ParseError)
    }

    fn get_path(&self) -> Result<String, PubkyError> {
        if self.is_pubky_url() {
            let after_protocol = &self[8..]; // Remove "pubky://"
            let parts: Vec<&str> = after_protocol.splitn(2, '/').collect();

            if parts.len() > 1 {
                Ok(format!("/{}", parts[1]))
            } else {
                Ok("/".to_string())
            }
        } else if self.is_pkarr_domain() {
            let url = Url::parse(self)?;
            Ok(url.path().to_string())
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }

    fn to_https_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_url() {
            // Convert pubky://public_key/path to https://public_key/path
            let https_url = format!("https{}", &self[5..]); // Replace "pubky" with "https"
            Url::parse(&https_url).map_err(PubkyError::ParseError)
        } else if self.is_pkarr_domain() {
            // Already an HTTPS URL
            Url::parse(self).map_err(PubkyError::ParseError)
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }
}

impl PubkyUrl for Url {
    fn is_pubky_url(&self) -> bool {
        self.scheme() == "pubky"
    }

    fn is_icann_url(&self) -> bool {
        PublicKey::try_from(self.as_str()).is_err()
    }

    fn is_pkarr_domain(&self) -> bool {
        if self.scheme() == "https" {
            if let Some(host) = self.host_str() {
                let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);
                return PublicKey::try_from(clean_host).is_ok();
            }
        }
        false
    }

    fn extract_public_key(&self) -> Result<PublicKey, PubkyError> {
        if self.is_pubky_url() {
            let host = self.host_str().ok_or(PubkyError::MissingHost)?;

            PublicKey::try_from(host).map_err(|_| PubkyError::InvalidPublicKey(host.to_string()))
        } else if self.is_pkarr_domain() {
            let host = self.host_str().ok_or(PubkyError::MissingHost)?;

            let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);

            PublicKey::try_from(clean_host)
                .map_err(|_| PubkyError::InvalidPublicKey(clean_host.to_string()))
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }

    fn to_homeserver_url(&self) -> Result<Url, PubkyError> {
        let homeserver_url = format!("https://_pubky.{}", self.as_str().split_at(8).1);
        Url::parse(&homeserver_url).map_err(PubkyError::ParseError)
    }

    fn get_path(&self) -> Result<String, PubkyError> {
        if self.is_pubky_related() {
            Ok(self.path().to_string())
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }

    fn to_https_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_url() {
            let mut https_url = self.clone();
            https_url
                .set_scheme("https")
                .map_err(|_| PubkyError::InvalidFormat("Cannot set HTTPS scheme".to_string()))?;
            Ok(https_url)
        } else if self.is_pkarr_domain() {
            Ok(self.clone())
        } else {
            Err(PubkyError::NotPubkyUrl)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pubky_url_detection() {
        let pubky_url =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let pkarr_url =
            "https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let homeserver_url = "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let regular_url = "https://example.com/file.txt";

        assert!(pubky_url.is_pubky_url());
        assert!(!pubky_url.is_pkarr_domain());
        assert!(pubky_url.is_pubky_related());

        assert!(!pkarr_url.is_pubky_url());
        assert!(pkarr_url.is_pkarr_domain());
        assert!(pkarr_url.is_pubky_related());

        assert!(!homeserver_url.is_pubky_url());
        assert!(homeserver_url.is_pkarr_domain());
        assert!(homeserver_url.is_pubky_related());

        assert!(!regular_url.is_pubky_url());
        assert!(!regular_url.is_pkarr_domain());
        assert!(!regular_url.is_pubky_related());
    }

    #[test]
    fn test_path_extraction() {
        let pubky_url =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let path = pubky_url.get_path().unwrap();
        assert_eq!(path, "/pub/example.com/file.txt");

        let pubky_url_no_path = "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy";
        let path = pubky_url_no_path.get_path().unwrap();
        assert_eq!(path, "/");
    }

    #[test]
    fn test_homeserver_conversion() {
        let pubky_url =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let homeserver = pubky_url.to_homeserver_url().unwrap();
        assert_eq!(
            homeserver.as_str(),
            "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt"
        );
    }
}
