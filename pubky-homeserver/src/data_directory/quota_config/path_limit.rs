use super::{HttpMethod, LimitKey, PathRegex, QuotaValue};
use http::Method;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// A limit on a path for a specific method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct PathLimit {
    /// The path regex pattern to match against.
    pub path: PathRegex,
    /// The method to limit.
    pub method: HttpMethod,
    /// The limit to apply.
    pub quota: QuotaValue,
    /// The key to limit.
    pub key: LimitKey,
    /// The burst to apply.
    pub burst: Option<NonZeroU32>,
}

impl PathLimit {
    /// Create a new path limit.
    pub fn new(
        path: Regex,
        method: Method,
        quota: QuotaValue,
        key: LimitKey,
        burst: Option<NonZeroU32>,
    ) -> Self {
        Self {
            path: PathRegex(path),
            method: HttpMethod(method),
            quota,
            key,
            burst,
        }
    }
}

impl std::fmt::Display for PathLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}: {}-{} by {}",
            self.method,
            self.path,
            self.quota,
            self.burst.map_or_else(String::new, |b| b.to_string()),
            self.key
        )
    }
}

impl From<PathLimit> for governor::Quota {
    fn from(value: PathLimit) -> Self {
        let quota: governor::Quota = value.quota.into();
        if let Some(burst) = value.burst {
            quota.allow_burst(burst);
        }
        quota
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_serde() {
        let method = Method::GET;
        let http_method = HttpMethod(method);
        assert_eq!(http_method.to_string(), "GET");

        let deserialized: HttpMethod = "GET".parse().unwrap();
        assert_eq!(deserialized, http_method);
    }

    #[test]
    fn test_path_regex_serde() {
        let regex = Regex::new(r"^/api/v1/users/\d+$").unwrap();
        let path_regex = PathRegex(regex);
        assert_eq!(path_regex.to_string(), r"^/api/v1/users/\d+$");

        let deserialized: PathRegex = r"^/api/v1/users/\d+$".parse().unwrap();
        assert_eq!(deserialized, path_regex);
    }

    #[test]
    fn test_path_regex_matching() {
        let path_regex: PathRegex = r"^/api/v1/users/\d+$".parse().unwrap();
        assert!(path_regex.0.is_match("/api/v1/users/123"));
        assert!(!path_regex.0.is_match("/api/v1/users/abc"));
    }

    #[test]
    fn test_path_regex_matching2() {
        let path_regex: PathRegex = r"^/pub/.*$".parse().unwrap();
        assert!(path_regex.0.is_match("/pub/user_pubky/file.txt"));
        assert!(path_regex.0.is_match("/pub/"));
    }
}
