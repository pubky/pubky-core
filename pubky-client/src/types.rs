use crate::errors::PubkyError;
use pkarr::PublicKey;
use std::fmt;
use url::Url;

#[allow(dead_code)]
pub struct PubkyUrl {
    public_key: PublicKey,
    url: Url, // RFC compliant URL
}

#[allow(dead_code)]
impl PubkyUrl {
    /// Creates a new PubkyUrl
    pub fn new(public_key: PublicKey, url: Url) -> Self {
        Self { public_key, url }
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
        Ok(PubkyUrl { public_key, url })
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
    pub fn from_parts(public_key: PublicKey, url: Url) -> Self {
        Self::new(public_key, url)
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

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const TEST_PUBLIC_KEY: &str = "operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";

    // Helper function to create a test PublicKey
    fn test_public_key() -> PublicKey {
        PublicKey::try_from(TEST_PUBLIC_KEY).expect("Valid test public key")
    }

    // Test PubkyUrl::new
    #[test]
    fn test_new_pubky_url() {
        let public_key = test_public_key();
        let url = Url::parse("pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json").unwrap();
        let pubky_url = PubkyUrl::new(public_key.clone(), url.clone());

        assert_eq!(pubky_url.public_key(), &public_key);
        assert_eq!(pubky_url.url(), &url);
    }

    // Test from_url_str with pubky:// scheme
    #[test]
    fn test_from_url_str_pubky_scheme() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse valid pubky URL");

        assert_eq!(pubky_url.public_key().to_string(), TEST_PUBLIC_KEY);
        assert_eq!(pubky_url.scheme(), "pubky");
        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
    }

    #[test]
    fn test_from_url_str_https_scheme_with_pubky_prefix() {
        let url_str = "https://_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse valid https _pubky URL");

        assert_eq!(pubky_url.public_key().to_string(), TEST_PUBLIC_KEY);
        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
    }

    #[test]
    fn test_from_url_str_https_scheme_without_prefix() {
        let url_str = "https://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse valid https URL");

        assert_eq!(pubky_url.public_key().to_string(), TEST_PUBLIC_KEY);
        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
    }

    // Test various resource types
    #[test]
    fn test_from_url_str_user_profile() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse user profile URL");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
    }

    #[test]
    fn test_from_url_str_post() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/0032SSN7Q4EVG";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse post URL");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/posts/0032SSN7Q4EVG");
    }

    #[test]
    fn test_from_url_str_follow() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/follows/operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse follow URL");

        assert_eq!(
            pubky_url.path(),
            "/pub/pubky.app/follows/operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo"
        );
    }

    #[test]
    fn test_from_url_str_bookmark() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/bookmarks/8Z8CWH8NVYQY39ZEBFGKQWWEKG";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse bookmark URL");

        assert_eq!(
            pubky_url.path(),
            "/pub/pubky.app/bookmarks/8Z8CWH8NVYQY39ZEBFGKQWWEKG"
        );
    }

    #[test]
    fn test_from_url_str_tag() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/tags/8Z8CWH8NVYQY39ZEBFGKQWWEKG";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse tag URL");

        assert_eq!(
            pubky_url.path(),
            "/pub/pubky.app/tags/8Z8CWH8NVYQY39ZEBFGKQWWEKG"
        );
    }

    #[test]
    fn test_from_url_str_file() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/files/file003";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse file URL");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/files/file003");
    }

    #[test]
    fn test_from_url_str_blob() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/blobs/8Z8CWH8NVYQY39ZEBFGKQWWEKG";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse blob URL");

        assert_eq!(
            pubky_url.path(),
            "/pub/pubky.app/blobs/8Z8CWH8NVYQY39ZEBFGKQWWEKG"
        );
    }

    #[test]
    fn test_from_url_str_feed() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/feeds/8Z8CWH8NVYQY39ZEBFGKQWWEKG";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse feed URL");

        assert_eq!(
            pubky_url.path(),
            "/pub/pubky.app/feeds/8Z8CWH8NVYQY39ZEBFGKQWWEKG"
        );
    }

    #[test]
    fn test_from_url_str_last_read() {
        let url_str =
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/last_read";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse last_read URL");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/last_read");
    }

    // Test URLs with query parameters and fragments
    #[test]
    fn test_from_url_str_with_query() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/123?limit=10&offset=20";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with query");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/posts/123");
        assert_eq!(pubky_url.query(), Some("limit=10&offset=20"));
    }

    #[test]
    fn test_from_url_str_with_fragment() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/123#section1";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with fragment");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/posts/123");
        assert_eq!(pubky_url.fragment(), Some("section1"));
    }

    #[test]
    fn test_from_url_str_with_query_and_fragment() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/123?limit=10#section1";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with query and fragment");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/posts/123");
        assert_eq!(pubky_url.query(), Some("limit=10"));
        assert_eq!(pubky_url.fragment(), Some("section1"));
    }

    // Test getter methods
    #[test]
    fn test_getters() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL");

        assert_eq!(pubky_url.scheme(), "pubky");
        assert_eq!(
            pubky_url.host(),
            Some("operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo")
        );
        assert_eq!(pubky_url.port(), None);
        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
        assert_eq!(pubky_url.query(), None);
        assert_eq!(pubky_url.fragment(), None);
        assert_eq!(pubky_url.url_str(), url_str);
    }

    // Test conversion methods
    #[test]
    fn test_to_rfc_url() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL");
        let rfc_url = pubky_url.to_rfc_url();

        assert_eq!(rfc_url.scheme(), "https");
        assert_eq!(
            rfc_url.host_str(),
            Some("_pubky.operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo")
        );
        assert_eq!(rfc_url.path(), "/pub/pubky.app/profile.json");
    }

    #[test]
    fn test_as_uri_string() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL");
        let uri_string = pubky_url.as_uri_string();

        assert_eq!(uri_string, url_str);
    }

    #[test]
    fn test_into_parts() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse URL");
        let (public_key, url) = pubky_url.into_parts();

        assert_eq!(public_key.to_string(), TEST_PUBLIC_KEY);
        assert_eq!(url.as_str(), url_str);
    }

    #[test]
    fn test_from_parts() {
        let public_key = test_public_key();
        let url = Url::parse("pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json").unwrap();
        let pubky_url = PubkyUrl::from_parts(public_key.clone(), url.clone());

        assert_eq!(pubky_url.public_key(), &public_key);
        assert_eq!(pubky_url.url(), &url);
    }

    // Test edge cases and empty paths
    #[test]
    fn test_empty_path() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with empty path");

        assert_eq!(pubky_url.path(), "/");
    }

    #[test]
    fn test_root_path() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with root path");

        assert_eq!(pubky_url.path(), "");
    }

    #[test]
    fn test_pub_app_base_path() {
        let url_str = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/";
        let pubky_url = PubkyUrl::from_url_str(url_str).expect("Failed to parse base app URL");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/");
    }

    // Test error cases
    #[test]
    fn test_invalid_scheme() {
        let url_str = "http://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/profile.json";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PubkyError::NotIntoPubkyUrl));
    }

    #[test]
    fn test_invalid_public_key() {
        let url_str = "pubky://invalid_public_key/pub/pubky.app/profile.json";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PubkyError::InvalidPublicKey(_)
        ));
    }

    /*#[test]
    fn test_missing_host() {
        let url_str = "pubky:///pub/pubky.app/profile.json";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PubkyError::InvalidUrl(_)));
    }*/

    #[test]
    fn test_invalid_url() {
        let url_str = "not a url";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PubkyError::NotIntoPubkyUrl));
    }

    /*#[test]
    fn test_https_missing_host() {
        let url_str = "https:///pub/pubky.app/profile.json";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), PubkyError::NotIntoPubkyUrl));
    }*/

    #[test]
    fn test_https_invalid_public_key_in_host() {
        let url_str = "https://invalid_public_key/pub/pubky.app/profile.json";
        let result = PubkyUrl::from_url_str(url_str);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PubkyError::InvalidPublicKey(_)
        ));
    }

    // Test special cases for empty bookmark paths
    #[test]
    fn test_empty_bookmark_path() {
        let url_str =
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/bookmarks/";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse empty bookmark path");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/bookmarks/");
    }

    #[test]
    fn test_bookmarks_base_path() {
        let url_str =
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/bookmarks";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse bookmarks base path");

        assert_eq!(pubky_url.path(), "/pub/pubky.app/bookmarks");
    }

    // Test URL round-trip conversion
    #[test]
    fn test_round_trip_conversion() {
        let original_url = "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/123?limit=10#section1";
        let pubky_url = PubkyUrl::from_url_str(original_url).expect("Failed to parse URL");
        let converted_back = pubky_url.as_uri_string();

        // Note: The as_uri_string method only includes scheme, host, and path
        let expected =
            "pubky://operrr8wsbpr3ue9d4qj41ge1kcc6r7fdiy6o3ugjrrhi4y77rdo/pub/pubky.app/posts/123";
        assert_eq!(converted_back, expected);
    }

    // Test with different public key formats
    #[test]
    fn test_different_public_key_format() {
        let url_str = "pubky://8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo/pub/pubky.app/profile.json";
        let pubky_url =
            PubkyUrl::from_url_str(url_str).expect("Failed to parse URL with different public key");

        assert_eq!(
            pubky_url.public_key().to_string(),
            "8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo"
        );
        assert_eq!(pubky_url.path(), "/pub/pubky.app/profile.json");
    }

    // Test URL detection and classification
    #[test]
    fn test_pubky_url_detection() {
        let pubky_url_str =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let pkarr_url_str =
            "https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let homeserver_url_str = "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let regular_url_str = "https://example.com/file.txt";

        // Test pubky:// URL
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str).expect("Should parse pubky URL");
        assert_eq!(pubky_url.scheme(), "pubky");
        assert_eq!(pubky_url.path(), "/pub/example.com/file.txt");

        // Test https with pkarr domain
        let pkarr_url = PubkyUrl::from_url_str(pkarr_url_str).expect("Should parse pkarr URL");
        assert_eq!(
            pkarr_url.public_key().to_string(),
            "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
        );
        assert_eq!(pkarr_url.path(), "/pub/example.com/file.txt");

        // Test https with _pubky prefix
        let homeserver_url =
            PubkyUrl::from_url_str(homeserver_url_str).expect("Should parse homeserver URL");
        assert_eq!(
            homeserver_url.public_key().to_string(),
            "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy"
        );
        assert_eq!(homeserver_url.path(), "/pub/example.com/file.txt");

        // Test regular URL (should fail)
        let regular_result = PubkyUrl::from_url_str(regular_url_str);
        assert!(
            regular_result.is_err(),
            "Regular URL should not parse as PubkyUrl"
        );
    }

    #[test]
    fn test_path_extraction() {
        let pubky_url_str =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str).expect("Should parse pubky URL");
        let path = pubky_url.path();
        assert_eq!(path, "/pub/example.com/file.txt");
    }

    #[test]
    fn test_homeserver_conversion() {
        let pubky_url_str =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str).expect("Should parse pubky URL");
        let homeserver = pubky_url.to_rfc_url();
        assert_eq!(
            homeserver.as_str(),
            "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt"
        );
    }

    #[test]
    fn test_homeserver_conversion_with_query_and_fragment() {
        let pubky_url_str = "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt?param=value#section";
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str)
            .expect("Should parse pubky URL with query and fragment");
        let homeserver = pubky_url.to_rfc_url();

        // Note: to_rfc_url() only includes scheme, host, and path based on the implementation
        assert_eq!(
            homeserver.as_str(),
            "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt"
        );

        // Original URL should still have query and fragment
        assert_eq!(pubky_url.query(), Some("param=value"));
        assert_eq!(pubky_url.fragment(), Some("section"));
    }

    #[test]
    fn test_uri_string_conversion() {
        let pubky_url_str =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str).expect("Should parse pubky URL");
        let uri_string = pubky_url.as_uri_string();
        assert_eq!(uri_string, pubky_url_str);
    }

    #[test]
    fn test_uri_string_conversion_strips_query_and_fragment() {
        let pubky_url_str = "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt?param=value#section";
        let pubky_url = PubkyUrl::from_url_str(pubky_url_str)
            .expect("Should parse pubky URL with query and fragment");
        let uri_string = pubky_url.as_uri_string();

        // as_uri_string() only includes scheme, host, and path based on the implementation
        let expected =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        assert_eq!(uri_string, expected);
    }

    #[test]
    fn test_conversion_between_formats() {
        let original_pubky =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let expected_https = "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";

        // Convert pubky:// to https://_pubky.
        let pubky_url = PubkyUrl::from_url_str(original_pubky).expect("Should parse pubky URL");
        let https_url = pubky_url.to_rfc_url();
        assert_eq!(https_url.as_str(), expected_https);

        // Convert https://_pubky. back to pubky://
        let pubky_url_from_https =
            PubkyUrl::from_url_str(expected_https).expect("Should parse https _pubky URL");
        let pubky_uri = pubky_url_from_https.as_uri_string();
        assert_eq!(pubky_uri, original_pubky);
    }

    #[test]
    fn test_pkarr_domain_conversion() {
        let pkarr_url_str =
            "https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let expected_pubky =
            "pubky://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";
        let expected_homeserver = "https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy/pub/example.com/file.txt";

        let pubky_url =
            PubkyUrl::from_url_str(pkarr_url_str).expect("Should parse pkarr domain URL");

        // Convert to pubky:// format
        let pubky_uri = pubky_url.as_uri_string();
        assert_eq!(pubky_uri, expected_pubky);

        // Convert to homeserver format
        let homeserver_url = pubky_url.to_rfc_url();
        assert_eq!(homeserver_url.as_str(), expected_homeserver);
    }
}

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
    fn to_homeserver_url(&self) -> Result<Url, PubkyError>;

    /// Get the path component after the public key
    fn get_path(&self) -> Result<String, PubkyError>;

    /// Convert pubky:// URL to https:// equivalent
    fn to_https_url(&self) -> Result<Url, PubkyError>;
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

    fn to_homeserver_url(&self) -> Result<Url, PubkyError> {
        let public_key = self.extract_public_key()?;
        let path = self.get_path()?;

        let homeserver_url = if path.is_empty() || path == "/" {
            format!("https://_pubky.{}/", public_key)
        } else {
            format!("https://_pubky.{}{}", public_key, path)
        };

        Url::parse(&homeserver_url).map_err(PubkyError::UrlParseError)
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

    fn to_https_url(&self) -> Result<Url, PubkyError> {
        if self.is_pubky_url() {
            // Convert pubky://public_key/path to https://public_key/path
            let https_url = format!("https{}", &self[5..]); // Replace "pubky" with "https"
            Url::parse(&https_url).map_err(PubkyError::UrlParseError)
        } else if self.is_pkarr_domain() {
            // Already an HTTPS URL
            Url::parse(self).map_err(PubkyError::UrlParseError)
        } else {
            Err(PubkyError::NotIntoPubkyUrl)
        }
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

    fn to_homeserver_url(&self) -> Result<Url, PubkyError> {
        let homeserver_url = format!("https://_pubky.{}", self.as_str().split_at(8).1);
        Url::parse(&homeserver_url).map_err(PubkyError::UrlParseError)
    }

    fn get_path(&self) -> Result<String, PubkyError> {
        if self.is_pubky_related() {
            Ok(self.path().to_string())
        } else {
            Err(PubkyError::NotIntoPubkyUrl)
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
            Err(PubkyError::NotIntoPubkyUrl)
        }
    }
}
