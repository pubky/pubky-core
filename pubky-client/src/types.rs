use crate::errors::PubkyError;
use pkarr::PublicKey;
use std::fmt;
use url::Url;

#[allow(dead_code)]
pub struct PubkyUrl {
    testnet: bool,
    public_key: PublicKey,
    url: Url, // RFC compliant URL
}

#[allow(dead_code)]
impl PubkyUrl {
    fn new(testnet: bool, public_key: PublicKey, url: Url) -> PubkyUrl {
        Self {
            testnet,
            public_key,
            url,
        }
    }

    pub fn with_testnet_enabled(mut self) -> Self {
        self.testnet = true;
        self
    }

    pub fn testnet(&self) -> bool {
        self.testnet
    }

    pub fn set_testnet(&mut self, value: bool) {
        self.testnet = value
    }

    /// Creates a PubkyUrl from a URL string
    pub fn from_url_str(url_str: &str) -> Result<Self, PubkyError> {
        let (public_key, url) = match Url::parse(url_str) {
            Ok(u) if u.scheme() == "https" => {
                if let Some(host) = u.host_str() {
                    // Remove _pubky. prefix if present for validation
                    let clean_host = host.strip_prefix("_pubky.").unwrap_or(host);
                    let public_key = PublicKey::try_from(clean_host)
                        .map_err(|_| PubkyError::InvalidPublicKey(u.to_string()))?;
                    (public_key, u)
                } else {
                    return Err(PubkyError::NotIntoPubkyUrl);
                }
            }
            Ok(u) if u.scheme() == "pubky" => {
                // Extract public key from pubky://public_key/path format
                let after_protocol = &url_str[8..]; // Remove "pubky://"
                let public_key_part = after_protocol
                    .split('/')
                    .next()
                    .ok_or_else(|| PubkyError::InvalidUrl(url_str.to_string()))?;
                let public_key = PublicKey::try_from(public_key_part)
                    .map_err(|_| PubkyError::InvalidPublicKey(public_key_part.to_string()))?;
                (public_key, u)
            }
            _ => return Err(PubkyError::NotIntoPubkyUrl),
        };
        Ok(PubkyUrl {
            testnet: true,
            public_key,
            url,
        })
    }

    /// Getter for public_key
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Getter for url
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Getter for url as string
    pub fn url_str(&self) -> &str {
        self.url.as_str()
    }

    /// Gets the scheme of the URL
    pub fn scheme(&self) -> &str {
        "pubky"
    }

    /// Gets the host of the URL
    pub fn host(&self) -> Option<&str> {
        self.url.host_str()
    }

    /// Gets the port of the URL
    pub fn port(&self) -> Option<u16> {
        self.url.port()
    }

    /// Gets the path of the URL
    pub fn path(&self) -> &str {
        self.url.path()
    }

    /// Gets the query string of the URL
    pub fn query(&self) -> Option<&str> {
        self.url.query()
    }

    /// Gets the fragment of the URL
    pub fn fragment(&self) -> Option<&str> {
        self.url.fragment()
    }

    /// Converts to a tuple of (PublicKey, Url)
    pub fn into_parts(self) -> (PublicKey, Url) {
        (self.public_key, self.url)
    }

    /// Creates a PubkyUrl from parts
    pub fn from_parts(testnet: bool, public_key: PublicKey, url: Url) -> Self {
        Self::new(testnet, public_key, url)
    }
    fn to_rfc_url(&self) -> Url {
        Url::parse(format!("https://_pubky.{}{}", self.public_key(), self.path()).as_str())
            .expect("Valid URL object expected")
    }

    fn as_uri_string(&self) -> String {
        format!("pubky://{}{}", self.public_key(), self.path()).clone()
    }
}

// Standard trait implementations
impl Clone for PubkyUrl {
    fn clone(&self) -> Self {
        Self {
            testnet: self.testnet,
            public_key: self.public_key.clone(),
            url: self.url.clone(),
        }
    }
}

impl fmt::Debug for PubkyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PubkyUrl")
            .field("public_key", &self.public_key)
            .field("url", &self.url)
            .finish()
    }
}

impl fmt::Display for PubkyUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PubkyUrl {{ public_key: {:?}, url: {} }}",
            self.public_key, self.url
        )
    }
}

impl PartialEq for PubkyUrl {
    fn eq(&self, other: &Self) -> bool {
        self.public_key == other.public_key && self.url == other.url
    }
}

impl Eq for PubkyUrl {}

impl std::hash::Hash for PubkyUrl {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.public_key.hash(state);
        self.url.hash(state);
    }
}
// Test getter methods

pub trait IntoPubkyUrl {
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
    fn to_url(&self) -> Result<Url, PubkyError>;

    /// Get the path component after the public key
    fn get_path(&self) -> Result<String, PubkyError>;

    fn get_query(&self) -> Result<String, PubkyError>;
}

impl IntoPubkyUrl for &str {
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
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }

    fn to_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_url() {
            let public_key = self.extract_public_key()?;
            let path = self.get_path()?;
            let homeserver_url = if path.is_empty() || path == "/" {
                format!("https://_pubky.{}/", public_key)
            } else {
                format!("https://_pubky.{}{}", public_key, path)
            };
            Url::parse(&homeserver_url).map_err(PubkyError::UrlParseError)
        } else if self.is_pkarr_domain() {
            // Already an HTTPS URL
            Url::parse(self).map_err(PubkyError::UrlParseError)
        } else {
            Url::parse(self).map_err(PubkyError::UrlParseError)
        }
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
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }

    fn get_query(&self) -> Result<String, PubkyError> {
        todo!()
    }
}

impl IntoPubkyUrl for Url {
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
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }

    fn to_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_url() {
            let public_key = self.extract_public_key()?;
            let path = self.get_path()?;
            let query = self.get_query()?;
            let homeserver_url = if path.is_empty() || path == "/" {
                format!("https://_pubky.{}?{}", public_key, query)
            } else {
                format!("https://_pubky.{}{}?{}", public_key, path, query)
            };
            Url::parse(&homeserver_url).map_err(PubkyError::UrlParseError)
        } else if self.is_pkarr_domain() {
            // Already an HTTPS URL
            Url::parse(self.as_str()).map_err(PubkyError::UrlParseError)
        } else {
            Url::parse(self.as_str()).map_err(PubkyError::UrlParseError)
        }
    }

    fn get_path(&self) -> Result<String, PubkyError> {
        if self.is_pubky_related() {
            Ok(self.path().to_string())
        } else {
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }

    fn get_query(&self) -> Result<String, PubkyError> {
        if self.is_pubky_related() {
            Ok(self.query().unwrap_or("").to_string())
        } else {
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }
}
