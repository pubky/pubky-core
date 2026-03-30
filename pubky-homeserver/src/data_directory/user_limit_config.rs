use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use pubky_common::crypto::PublicKey;
use serde::{Deserialize, Serialize};

use super::config_toml::GeneralToml;
use crate::data_directory::quota_config::BandwidthBudget;

/// How long a cached limit entry is considered fresh before re-resolving from DB.
const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// Maximum number of entries in the user limits cache. Prevents unbounded memory
/// growth from requests for many distinct users between periodic cleanup sweeps.
pub const MAX_CACHED_USER_LIMITS: usize = 100_000;

/// A cached user limit config with an expiry timestamp.
#[derive(Debug, Clone)]
pub struct CachedUserLimits {
    /// The resolved limit configuration.
    pub config: UserLimitConfig,
    cached_at: Instant,
}

impl CachedUserLimits {
    /// Wrap a resolved config with a fresh timestamp.
    pub fn new(config: UserLimitConfig) -> Self {
        Self {
            config,
            cached_at: Instant::now(),
        }
    }

    /// Returns true if this entry has exceeded the cache TTL.
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > CACHE_TTL
    }
}

/// Shared cache for resolved per-user limits.
/// Used by both admin (for eviction on PUT/DELETE) and client (for resolution) servers.
/// Entries expire after [`CACHE_TTL`] and are re-resolved from the database.
pub type UserLimitsCache = Arc<DashMap<PublicKey, CachedUserLimits>>;

/// Per-user resource limits. `None` fields mean "unlimited / no limit".
///
/// Used in three contexts:
/// 1. **Deploy-time defaults** — parsed from TOML config via [`UserLimitConfig::from_general_toml`].
/// 2. **Per-user config** — stored on the user row in the DB. When present, replaces defaults entirely.
/// 3. **Signup token config** — attached to a signup code; applied to the user on signup.
///
/// There is no merging: if a user has a custom config, it is used as-is. If not, deploy-time
/// defaults apply. Within a config, each `None` field means "unlimited".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserLimitConfig {
    /// Maximum storage in MB. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_quota_mb: Option<u64>,
    /// Maximum concurrent sessions. `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_sessions: Option<u32>,
    /// Per-user read bandwidth budget (e.g. "500mb/d"). `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_read: Option<BandwidthBudget>,
    /// Per-user write bandwidth budget (e.g. "100mb/h"). `None` = unlimited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_write: Option<BandwidthBudget>,
}

impl UserLimitConfig {
    /// Construct default limits from the general config section.
    ///
    /// For backward compatibility, the deprecated `user_storage_quota_mb = 0` is
    /// treated as unlimited (`None`). The newer `storage_limit_mb` field uses
    /// `Option` directly — `Some(0)` means "zero quota", not unlimited.
    ///
    /// The result of this function is used both as the runtime default for new
    /// users and as the backfill value in [`M20260327AddUserLimitColumnsMigration`],
    /// which "freezes" these defaults onto existing user rows during the one-time
    /// migration. See that migration's docs for details.
    pub fn from_general_toml(general: &GeneralToml) -> Self {
        Self {
            storage_quota_mb: general
                .storage_limit_mb
                .or(match general.user_storage_quota_mb {
                    0 => None,
                    n => Some(n),
                }),
            max_sessions: general.max_sessions,
            rate_read: general.user_rate_read.clone(),
            rate_write: general.user_rate_write.clone(),
        }
    }

    /// Convert nullable DB columns into an `Option<UserLimitConfig>`.
    /// Returns `None` when all columns are NULL (user has no custom config; use defaults).
    /// If any column is non-NULL, returns `Some` — NULL fields within that mean "unlimited".
    pub fn from_nullable_columns(
        storage_quota_mb: Option<i64>,
        max_sessions: Option<i32>,
        rate_read: Option<String>,
        rate_write: Option<String>,
    ) -> Option<Self> {
        if storage_quota_mb.is_none()
            && max_sessions.is_none()
            && rate_read.is_none()
            && rate_write.is_none()
        {
            return None;
        }
        Some(Self {
            storage_quota_mb: storage_quota_mb.and_then(|v| {
                u64::try_from(v)
                    .map_err(|_| {
                        tracing::warn!(
                            "Negative limit_storage_quota_mb ({v}) in DB; treating as zero quota"
                        );
                    })
                    .ok()
            }),
            max_sessions: max_sessions.and_then(|v| {
                u32::try_from(v)
                    .map_err(|_| {
                        tracing::warn!(
                            "Negative limit_max_sessions ({v}) in DB; treating as zero"
                        );
                    })
                    .ok()
            }),
            rate_read: parse_rate_column("rate_read", rate_read),
            rate_write: parse_rate_column("rate_write", rate_write),
        })
    }

    /// Serialise rate fields to the string representation stored in DB columns.
    pub fn rate_read_str(&self) -> Option<String> {
        self.rate_read.as_ref().map(|v| v.to_string())
    }

    /// Serialise rate fields to the string representation stored in DB columns.
    pub fn rate_write_str(&self) -> Option<String> {
        self.rate_write.as_ref().map(|v| v.to_string())
    }
}

/// Parse a bandwidth budget string from a DB column into a [`BandwidthBudget`].
/// Logs a warning and returns `None` if the string is malformed (including
/// legacy request-unit strings like `"100r/m"`).
fn parse_rate_column(column: &str, value: Option<String>) -> Option<BandwidthBudget> {
    value.and_then(|s| {
        s.parse()
            .map_err(|e| {
                tracing::warn!("Invalid {column} \"{s}\" in DB: {e}; treating as unlimited");
            })
            .ok()
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_from_general_toml_new_fields() {
        let general = GeneralToml {
            storage_limit_mb: Some(500),
            max_sessions: Some(10),
            user_rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            user_rate_write: Some(BandwidthBudget::from_str("50mb/m").unwrap()),
            ..Default::default()
        };
        let config = UserLimitConfig::from_general_toml(&general);
        assert_eq!(config.storage_quota_mb, Some(500));
        assert_eq!(config.max_sessions, Some(10));
        assert_eq!(config.rate_read, Some(BandwidthBudget::from_str("100mb/m").unwrap()));
        assert_eq!(config.rate_write, Some(BandwidthBudget::from_str("50mb/m").unwrap()));
    }

    #[test]
    fn test_from_general_toml_deprecated_storage_fallback() {
        let general = GeneralToml {
            user_storage_quota_mb: 1024,
            ..Default::default()
        };
        let config = UserLimitConfig::from_general_toml(&general);
        assert_eq!(config.storage_quota_mb, Some(1024));
    }

    #[test]
    fn test_from_general_toml_new_field_takes_precedence() {
        let general = GeneralToml {
            user_storage_quota_mb: 100, // old
            storage_limit_mb: Some(500), // new takes precedence
            ..Default::default()
        };
        let config = UserLimitConfig::from_general_toml(&general);
        assert_eq!(config.storage_quota_mb, Some(500));
    }

    #[test]
    fn test_from_general_toml_deprecated_zero_is_unlimited() {
        let general = GeneralToml {
            user_storage_quota_mb: 0,
            ..Default::default()
        };
        let config = UserLimitConfig::from_general_toml(&general);
        assert_eq!(config.storage_quota_mb, None);
    }

    #[test]
    fn test_from_general_toml_all_defaults_unlimited() {
        let general = GeneralToml::default();
        let config = UserLimitConfig::from_general_toml(&general);
        assert_eq!(config, UserLimitConfig::default());
    }

    #[test]
    fn test_from_nullable_columns_all_null() {
        assert_eq!(
            UserLimitConfig::from_nullable_columns(None, None, None, None),
            None
        );
    }

    #[test]
    fn test_from_nullable_columns_with_values() {
        let config = UserLimitConfig::from_nullable_columns(
            Some(500),
            Some(10),
            Some("100mb/m".to_string()),
            None,
        );
        assert_eq!(
            config,
            Some(UserLimitConfig {
                storage_quota_mb: Some(500),
                max_sessions: Some(10),
                rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_all_negative() {
        let config =
            UserLimitConfig::from_nullable_columns(Some(-1), Some(-5), None, None);
        // Negative values fail try_from → None (unlimited), but a warning is logged.
        assert_eq!(
            config,
            Some(UserLimitConfig {
                storage_quota_mb: None,
                max_sessions: None,
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_mixed_negative_and_positive() {
        let config =
            UserLimitConfig::from_nullable_columns(Some(-1), Some(10), None, None);
        assert_eq!(
            config,
            Some(UserLimitConfig {
                storage_quota_mb: None, // negative → warning + None
                max_sessions: Some(10),
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_invalid_rate_string() {
        let config = UserLimitConfig::from_nullable_columns(
            None,
            None,
            Some("garbage".to_string()),
            Some("100mb/m".to_string()),
        );
        assert_eq!(
            config,
            Some(UserLimitConfig {
                storage_quota_mb: None,
                max_sessions: None,
                rate_read: None, // invalid string → warning + None (unlimited)
                rate_write: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_legacy_request_units_treated_as_unlimited() {
        // Legacy "100r/m" strings fail BandwidthBudget parse → None (with warning)
        let config = UserLimitConfig::from_nullable_columns(
            None,
            None,
            Some("100r/m".to_string()),
            Some("50r/s".to_string()),
        );
        assert_eq!(
            config,
            Some(UserLimitConfig {
                storage_quota_mb: None,
                max_sessions: None,
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = UserLimitConfig {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            rate_write: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: UserLimitConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_serde_none_fields_omitted() {
        let config = UserLimitConfig {
            storage_quota_mb: Some(500),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("max_sessions"));
        assert!(!json.contains("rate_read"));
    }

    #[test]
    fn test_serde_empty_json_is_all_unlimited() {
        let config: UserLimitConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config, UserLimitConfig::default());
    }

    #[test]
    fn test_serde_rejects_invalid_rate_string() {
        let json = r#"{"rate_read": "garbage"}"#;
        let result: Result<UserLimitConfig, _> = serde_json::from_str(json);
        assert!(result.is_err(), "Invalid rate string should fail deserialization");
    }
}
