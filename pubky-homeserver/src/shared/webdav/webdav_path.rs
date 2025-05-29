use std::str::FromStr;

/// A normalized and validated webdav path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WebDavPath {
    normalized_path: String,
}

impl WebDavPath {
    /// Create a new WebDavPath from a already normalized path.
    /// Make sure the path is 100% normalized, and valid before using this constructor.
    ///
    /// Use `WebDavPath::new` to create a new WebDavPath from an unnormalized path.
    pub fn new_unchecked(normalized_path: String) -> Self {
        Self {
            normalized_path: normalized_path.to_string(),
        }
    }

    /// Create a new WebDavPath from an unnormalized path.
    ///
    /// The path will be normalized and validated.
    pub fn new(unnormalized_path: &str) -> anyhow::Result<Self> {
        let normalized_path = normalize_and_validate_webdav_path(unnormalized_path)?;
        Ok(Self::new_unchecked(normalized_path))
    }

    #[allow(dead_code)]
    pub fn url_encode(&self) -> String {
        percent_encoding::utf8_percent_encode(self.normalized_path.as_str(), PATH_ENCODE_SET)
            .to_string()
    }

    pub fn as_str(&self) -> &str {
        self.normalized_path.as_str()
    }

    pub fn is_directory(&self) -> bool {
        self.normalized_path.ends_with('/')
    }

    /// Check if the path is a file.
    #[allow(dead_code)]
    pub fn is_file(&self) -> bool {
        !self.is_directory()
    }
}

impl std::fmt::Display for WebDavPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.normalized_path)
    }
}

impl FromStr for WebDavPath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

// Encode all non-unreserved characters, except '/'.
// See RFC3986, and https://en.wikipedia.org/wiki/Percent-encoding .
const PATH_ENCODE_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

/// Maximum length of a single path segment.
const MAX_WEBDAV_PATH_SEGMENT_LENGTH: usize = 255;
/// Maximum total length of a normalized WebDAV path.
const MAX_WEBDAV_PATH_TOTAL_LENGTH: usize = 4096;

/// Takes a path, normalizes and validates it.
/// Make sure to url decode the path before calling this function.
/// Inspired by https://github.com/messense/dav-server-rs/blob/740dae05ac2eeda8e2ea11fface3ab6d53b6705e/src/davpath.rs#L101
fn normalize_and_validate_webdav_path(path: &str) -> anyhow::Result<String> {
    // Ensure the path starts with '/'
    if !path.starts_with('/') {
        return Err(anyhow::anyhow!("Path must start with '/'"));
    }

    let is_dir = path.ends_with('/') || path.ends_with("..");
    // Split the path into segments, filtering out empty ones
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // Build the normalized path
    let mut normalized_segments = vec![];

    for segment in segments {
        // Check for segment length
        if segment.len() > MAX_WEBDAV_PATH_SEGMENT_LENGTH {
            return Err(anyhow::anyhow!(
                "Invalid path: Segment exceeds maximum length of {} characters. Segment: '{}'",
                MAX_WEBDAV_PATH_SEGMENT_LENGTH,
                segment
            ));
        }

        // Check for any ASCII control characters in the decoded segment
        if segment.chars().any(|c| c.is_control()) {
            return Err(anyhow::anyhow!(
                "Invalid path: ASCII control characters are not allowed in segments"
            ));
        }

        if segment == "." {
            continue;
        } else if segment == ".." {
            if normalized_segments.len() < 2 {
                return Err(anyhow::anyhow!("Failed to normalize path: '..'."));
            }
            normalized_segments.pop();
            normalized_segments.pop();
        } else {
            normalized_segments.push("/".to_string());
            normalized_segments.push(segment.to_string());
        }
    }

    if is_dir {
        normalized_segments.push("/".to_string());
    }
    let full_path = normalized_segments.join("");

    // Check for total path length
    if full_path.len() > MAX_WEBDAV_PATH_TOTAL_LENGTH {
        return Err(anyhow::anyhow!(
            "Invalid path: Total path length exceeds maximum of {} characters. Length: {}, Path: '{}'",
            MAX_WEBDAV_PATH_TOTAL_LENGTH,
            full_path.len(),
            full_path
        ));
    }

    Ok(full_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid_path(path: &str, expected: &str) {
        match normalize_and_validate_webdav_path(path) {
            Ok(path) => {
                assert_eq!(path, expected);
            }
            Err(e) => {
                assert!(
                    false,
                    "Path '{path}' is invalid. Should be '{expected}'. Error: {e}"
                );
            }
        };
    }

    fn assert_invalid_path(path: &str) {
        if let Ok(normalized_path) = normalize_and_validate_webdav_path(path) {
            assert!(
                false,
                "Invalid path '{path}' is valid. Normalized result: '{normalized_path}'"
            );
        }
    }

    #[test]
    fn test_slash_is_valid() {
        assert_valid_path("/", "/");
    }

    #[test]
    fn test_two_dots_is_valid() {
        assert_valid_path("/test/..", "/");
    }

    #[test]
    fn test_two_dots_in_the_middle_is_valid() {
        assert_valid_path("/test/../test", "/test");
    }

    #[test]
    fn test_two_dots_in_the_middle_with_slash_is_valid() {
        assert_valid_path("/test/../test/", "/test/");
    }

    #[test]
    fn test_two_dots_invalid() {
        assert_invalid_path("/..");
    }

    #[test]
    fn test_two_dots_twice_invalid() {
        assert_invalid_path("/test/../..");
    }

    #[test]
    fn test_two_slashes_is_valid() {
        assert_valid_path("//", "/");
    }

    #[test]
    fn test_two_slashes_in_the_middle_is_valid() {
        assert_valid_path("/test//test", "/test/test");
    }

    #[test]
    fn test_one_segment_is_valid() {
        assert_valid_path("/test", "/test");
    }

    #[test]
    fn test_one_segment_with_trailing_slash_is_valid() {
        assert_valid_path("/test/", "/test/");
    }

    #[test]
    fn test_two_segments_is_valid() {
        assert_valid_path("/test/test", "/test/test");
    }

    #[test]
    fn test_wildcard_is_valid() {
        assert_valid_path("/dav/file*.txt", "/dav/file*.txt");
    }

    #[test]
    fn test_two_slashes_in_the_middle_with_slash_is_valid() {
        assert_valid_path("/dav//folder/", "/dav/folder/");
    }

    #[test]
    fn test_script_tag_is_valid() {
        assert_valid_path("/dav/<script>", "/dav/<script>");
    }

    #[test]
    fn test_null_is_invalid() {
        assert_invalid_path("/dav/file\0");
    }

    #[test]
    fn test_empty_path_is_invalid() {
        assert_invalid_path("");
    }

    #[test]
    fn test_missing_root_slash1_is_invalid() {
        assert_invalid_path("test");
    }

    #[test]
    fn test_missing_root_slash2_is_invalid() {
        assert_invalid_path("test/");
    }

    #[test]
    fn test_invalid_path_test_over_test() {
        assert_invalid_path("test/test");
    }

    #[test]
    fn test_invalid_path_http_example_com_test() {
        assert_invalid_path("http://example.com/test");
    }

    #[test]
    fn test_invalid_path_backslash_test_backslash() {
        assert_invalid_path("\\test\\");
    }

    #[test]
    fn test_invalid_path_dot() {
        assert_invalid_path(".");
    }

    #[test]
    fn test_invalid_path_dot_dot() {
        assert_invalid_path("..");
    }

    #[test]
    fn test_invalid_windows_path() {
        assert_invalid_path("C:\\dav\\file");
    }

    #[test]
    fn test_valid_path_dav_uber() {
        assert_valid_path("/dav/über", "/dav/über");
    }

    #[test]
    fn test_url_encode() {
        let url_encoded = "/pub/file%25.txt";
        let url_decoded = percent_encoding::percent_decode_str(url_encoded)
            .decode_utf8()
            .unwrap()
            .to_string();
        let path = WebDavPath::new(url_decoded.as_str()).unwrap();
        let normalized = path.to_string();
        assert_eq!(normalized, "/pub/file%.txt");
        assert_eq!(path.url_encode(), url_encoded);
    }

    #[test]
    fn test_segment_too_long() {
        let long_segment = "a".repeat(MAX_WEBDAV_PATH_SEGMENT_LENGTH + 1);
        let path = format!("/prefix/{}/suffix", long_segment);
        assert_invalid_path(&path);
    }

    #[test]
    fn test_segment_max_length_is_valid() {
        let max_segment = "a".repeat(MAX_WEBDAV_PATH_SEGMENT_LENGTH);
        let path = format!("/prefix/{}/suffix", max_segment);
        let expected_path = path.clone(); // Expected path is the same as input if valid
        assert_valid_path(&path, &expected_path);
    }

    #[test]
    fn test_total_path_too_long() {
        let num_segments = MAX_WEBDAV_PATH_TOTAL_LENGTH; // This will create path like "/a/a/.../a"
        let segments: Vec<String> = std::iter::repeat("a".to_string())
            .take(num_segments)
            .collect();
        let path = format!("/{}", segments.join("/"));
        assert_invalid_path(&path);

        let almost_too_long_segment = "a".repeat(MAX_WEBDAV_PATH_TOTAL_LENGTH - 1); // e.g., if max is 10, this is 9 'a's
        let path_too_long = format!("/{}/b", almost_too_long_segment); // "/aaaaaaaaa/b" -> 1 + 9 + 1 + 1 = 12 > 10
        assert_invalid_path(&path_too_long);
    }
}
