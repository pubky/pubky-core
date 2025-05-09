use super::{LimitKey, QuotaValue};
use http::Method;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, str::FromStr};

/// A wrapper around http::Method to implement serde traits
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HttpMethod(pub Method);

impl From<Method> for HttpMethod {
    fn from(method: Method) -> Self {
        HttpMethod(method)
    }
}

impl From<HttpMethod> for Method {
    fn from(method: HttpMethod) -> Self {
        method.0
    }
}

impl FromStr for HttpMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Method::from_str(s.to_uppercase().as_str())
            .map(HttpMethod)
            .map_err(|_| format!("Invalid method: {}", s))
    }
}

impl Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for HttpMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for HttpMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        HttpMethod::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// A wrapper around regex::Regex to implement serde traits
#[derive(Debug, Clone)]
pub struct PathRegex(pub Regex);

impl std::hash::Hash for PathRegex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

impl From<Regex> for PathRegex {
    fn from(regex: Regex) -> Self {
        PathRegex(regex)
    }
}

impl From<PathRegex> for Regex {
    fn from(path_regex: PathRegex) -> Self {
        path_regex.0
    }
}

impl FromStr for PathRegex {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Regex::new(s)
            .map(PathRegex)
            .map_err(|e| format!("Invalid regex pattern: {}", e))
    }
}

impl Display for PathRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl Serialize for PathRegex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for PathRegex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PathRegex::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl PartialEq for PathRegex {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for PathRegex {}

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
}

impl PathLimit {
    /// Create a new path limit.
    pub fn new(path: Regex, method: Method, quota: QuotaValue, key: LimitKey) -> Self {
        Self {
            path: PathRegex(path),
            method: HttpMethod(method),
            quota,
            key,
        }
    }
}

impl std::fmt::Display for PathLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}: {}", self.method, self.path, self.quota)
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
