use super::{HttpMethod, LimitKey, GlobPattern, QuotaValue};
use axum::http::Method;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

/// A limit on a path for a specific method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct PathLimit {
    /// The path glob pattern to match against.
    pub path: GlobPattern,
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
        path: GlobPattern,
        method: Method,
        quota: QuotaValue,
        key: LimitKey,
        burst: Option<NonZeroU32>,
    ) -> Self {
        Self {
            path,
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
}
