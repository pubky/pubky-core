use super::{limit_key::LimitKey, GlobPattern, HttpMethod, LimitKeyType, QuotaValue};
use axum::http::Method;
use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use std::num::NonZeroU32;

/// Make sure all whitelist keys are of the same type as the limit key type.
fn serde_validate_path_limit(limit: &PathLimit) -> Result<(), serde_valid::validation::Error> {
    limit
        .validate()
        .map_err(|e| serde_valid::validation::Error::Custom(e.to_string()))?;
    Ok(())
}

/// A limit on a path for a specific method.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash, Validate)]
#[validate(custom = serde_validate_path_limit)]
pub struct PathLimit {
    /// The path glob pattern to match against.
    pub path: GlobPattern,
    /// The method to limit.
    pub method: HttpMethod,
    /// The limit to apply.
    pub quota: QuotaValue,
    /// The key to limit.
    pub key: LimitKeyType,
    /// The burst to apply.
    pub burst: Option<NonZeroU32>,
    /// The whitelist of keys to limit.
    #[serde(default)]
    pub whitelist: Vec<LimitKey>,
}

impl PathLimit {
    /// Create a new path limit.
    pub fn new(
        path: GlobPattern,
        method: Method,
        quota: QuotaValue,
        key: LimitKeyType,
        burst: Option<NonZeroU32>,
    ) -> Self {
        Self {
            path,
            method: HttpMethod(method),
            quota,
            key,
            burst,
            whitelist: vec![],
        }
    }

    /// Check if the key is whitelisted.
    pub fn is_whitelisted(&self, key: &LimitKey) -> bool {
        self.whitelist.iter().any(|k| k == key)
    }

    /// Validate the path limit.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(k) = self.whitelist.iter().find(|k| k.get_type() != self.key) {
            let should_type = self.key.to_string();
            let is_type = k.get_type().to_string();
            let msg = format!("Whitelist key type mismatch for '{k}'. Expected type '{should_type}' but got '{is_type}'. Full path limit: {self}");
            return Err(anyhow::anyhow!(msg));
        }
        Ok(())
    }
}

impl std::fmt::Display for PathLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let burst_str = self
            .burst
            .map(|b| format!(" burst {}", b))
            .unwrap_or("".to_string());
        let whitelist_str = if self.whitelist.is_empty() {
            "".to_string()
        } else {
            format!(
                " whitelist: {}",
                self.whitelist
                    .iter()
                    .map(|k| k.to_string())
                    .collect::<Vec<String>>()
                    .join(", ")
            )
        };
        write!(
            f,
            "{} {}: {}{burst_str} by {}.{whitelist_str}",
            self.method, self.path, self.quota, self.key,
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
    use std::{
        net::{IpAddr, Ipv4Addr},
        str::FromStr,
    };

    use pkarr::Keypair;

    use super::*;

    #[test]
    fn test_validate_path_limit() {
        let mut limit = PathLimit::new(
            GlobPattern::new("*"),
            Method::GET,
            QuotaValue::from_str("10r/s").unwrap(),
            LimitKeyType::Ip,
            None,
        );
        assert!(limit.validate().is_ok());
        limit
            .whitelist
            .push(LimitKey::Ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))));
        assert!(limit.validate().is_ok());
        limit
            .whitelist
            .push(LimitKey::User(Keypair::random().public_key()));
        assert!(limit.validate().is_err());
    }
}
