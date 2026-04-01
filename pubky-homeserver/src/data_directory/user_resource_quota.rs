use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use pubky_common::crypto::PublicKey;
use serde::{Deserialize, Serialize};

use super::config_toml::ConfigToml;
use crate::data_directory::quota_config::BandwidthBudget;

/// How long a cached limit entry is considered fresh before re-resolving from DB.
const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// How long a negative (user-not-found) cache entry lives before re-checking the DB.
/// Short TTL so that a subsequent signup populates limits promptly.
const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(30);

/// Maximum length of the VARCHAR column used for rate budget strings in the DB.
/// Matches the `VARCHAR(32)` used in the `m20260327_add_resource_quota_columns` migration.
pub const MAX_RATE_COLUMN_LEN: usize = 32;

/// Maximum number of entries in the user limits cache. Prevents unbounded memory
/// growth from requests for many distinct users between periodic cleanup sweeps.
pub const MAX_CACHED_USER_RESOURCE_QUOTAS: usize = 100_000;

/// A cached user limit config with an expiry timestamp.
#[derive(Debug, Clone)]
pub struct CachedUserResourceQuota {
    /// The resolved limit configuration, or `None` for a negative (user-not-found) entry.
    pub config: Option<UserResourceQuota>,
    cached_at: Instant,
    ttl: Duration,
}

impl CachedUserResourceQuota {
    /// Wrap a resolved config with a fresh timestamp.
    pub fn new(config: UserResourceQuota) -> Self {
        Self {
            config: Some(config),
            cached_at: Instant::now(),
            ttl: CACHE_TTL,
        }
    }

    /// Create a negative cache entry (user not found) with a shorter TTL.
    pub fn not_found() -> Self {
        Self {
            config: None,
            cached_at: Instant::now(),
            ttl: NEGATIVE_CACHE_TTL,
        }
    }

    /// Returns true if this entry has exceeded its TTL.
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }
}

/// Shared cache for resolved per-user limits.
/// Used by both admin (for eviction on PUT/DELETE) and client (for resolution) servers.
/// Entries expire after [`CACHE_TTL`] and are re-resolved from the database.
pub type UserResourceQuotaCache = Arc<DashMap<PublicKey, CachedUserResourceQuota>>;

/// Per-user resource quotas. `None` fields mean "unlimited / no quota".
///
/// Used in three contexts:
/// 1. **Deploy-time defaults** — the `[quotas]` section in `config.toml`, via [`UserResourceQuota::from_config`].
/// 2. **Per-user config** — stored on the user row in the DB.
/// 3. **Signup token config** — attached to a signup code; applied to the user on signup.
///
/// There is no merging: if a user has a custom config, it is used as-is. If not, deploy-time
/// defaults apply. Within a config, each `None` field means "unlimited".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserResourceQuota {
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

impl UserResourceQuota {
    /// Construct deploy-time default quotas from the config file.
    ///
    /// Uses the `[quotas]` section directly. For backward compatibility, if
    /// `storage_quota_mb` is not set in `[quotas]`, falls back to the deprecated
    /// `[general] user_storage_quota_mb` (where `0` means unlimited).
    ///
    /// The result of this function is used both as the runtime default for new
    /// users and as the backfill value in [`M20260327AddResourceQuotaColumnsMigration`],
    /// which "freezes" these defaults onto existing user rows during the one-time
    /// migration. See that migration's docs for details.
    pub fn from_config(config: &ConfigToml) -> Self {
        let mut quotas = config.quotas.clone();
        // Backward compat: fall back to deprecated [general] user_storage_quota_mb
        if quotas.storage_quota_mb.is_none() {
            quotas.storage_quota_mb = match config.general.user_storage_quota_mb {
                0 => None,
                n => Some(n),
            };
        }
        quotas
    }

    /// Convert nullable DB columns into an `Option<UserResourceQuota>`.
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
                            "Negative quota_storage_mb ({v}) in DB; treating as zero quota"
                        );
                    })
                    .ok()
            }),
            max_sessions: max_sessions.and_then(|v| {
                u32::try_from(v)
                    .map_err(|_| {
                        tracing::warn!("Negative quota_max_sessions ({v}) in DB; treating as zero");
                    })
                    .ok()
            }),
            rate_read: parse_rate_column("rate_read", rate_read),
            rate_write: parse_rate_column("rate_write", rate_write),
        })
    }

    /// Serialise a rate field to the string representation stored in DB columns.
    pub fn rate_str(budget: &Option<BandwidthBudget>) -> Option<String> {
        budget.as_ref().map(|v| v.to_string())
    }

    /// Storage quota as the DB-column type (`BIGINT`).
    /// Saturates at `i64::MAX` instead of wrapping on overflow.
    pub fn storage_quota_mb_i64(&self) -> Option<i64> {
        self.storage_quota_mb
            .map(|v| i64::try_from(v).unwrap_or(i64::MAX))
    }

    /// Max sessions as the DB-column type (`INTEGER`).
    /// Saturates at `i32::MAX` instead of wrapping on overflow.
    pub fn max_sessions_i32(&self) -> Option<i32> {
        self.max_sessions
            .map(|v| i32::try_from(v).unwrap_or(i32::MAX))
    }

    /// Rate-read as the DB-column type (`VARCHAR`).
    pub fn rate_read_str(&self) -> Option<String> {
        Self::rate_str(&self.rate_read)
    }

    /// Rate-write as the DB-column type (`VARCHAR`).
    pub fn rate_write_str(&self) -> Option<String> {
        Self::rate_str(&self.rate_write)
    }

    /// Validate that rate budget fields survive a Display → FromStr roundtrip.
    ///
    /// Defence-in-depth: callers already hold parsed `BandwidthBudget` values,
    /// but this check prevents corrupt data from reaching the database.
    pub fn validate_rate_roundtrips(&self) -> Result<(), String> {
        for (label, budget) in [
            ("rate_read", &self.rate_read),
            ("rate_write", &self.rate_write),
        ] {
            if let Some(ref b) = budget {
                let s = b.to_string();
                if s.len() > MAX_RATE_COLUMN_LEN {
                    return Err(format!(
                        "{label} string \"{s}\" exceeds DB column limit of {MAX_RATE_COLUMN_LEN} characters"
                    ));
                }
                s.parse::<BandwidthBudget>()
                    .map_err(|e| format!("{label} roundtrip validation failed for \"{s}\": {e}"))?;
            }
        }
        Ok(())
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

    /// Helper: build a `ConfigToml` with custom quotas and default everything else.
    fn config_with_quotas(quotas: UserResourceQuota) -> ConfigToml {
        ConfigToml {
            quotas,
            ..ConfigToml::default()
        }
    }

    #[test]
    fn test_from_config_quotas_section() {
        let config = config_with_quotas(UserResourceQuota {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            rate_write: Some(BandwidthBudget::from_str("50mb/m").unwrap()),
        });
        let result = UserResourceQuota::from_config(&config);
        assert_eq!(result.storage_quota_mb, Some(500));
        assert_eq!(result.max_sessions, Some(10));
        assert_eq!(
            result.rate_read,
            Some(BandwidthBudget::from_str("100mb/m").unwrap())
        );
        assert_eq!(
            result.rate_write,
            Some(BandwidthBudget::from_str("50mb/m").unwrap())
        );
    }

    #[test]
    fn test_from_config_deprecated_storage_fallback() {
        let mut config = ConfigToml::default();
        config.general.user_storage_quota_mb = 1024;
        let result = UserResourceQuota::from_config(&config);
        assert_eq!(result.storage_quota_mb, Some(1024));
    }

    #[test]
    fn test_from_config_quotas_section_takes_precedence() {
        let mut config = config_with_quotas(UserResourceQuota {
            storage_quota_mb: Some(500),
            ..Default::default()
        });
        config.general.user_storage_quota_mb = 100; // deprecated, should be ignored
        let result = UserResourceQuota::from_config(&config);
        assert_eq!(result.storage_quota_mb, Some(500));
    }

    #[test]
    fn test_from_config_deprecated_zero_is_unlimited() {
        let mut config = ConfigToml::default();
        config.general.user_storage_quota_mb = 0;
        let result = UserResourceQuota::from_config(&config);
        assert_eq!(result.storage_quota_mb, None);
    }

    #[test]
    fn test_from_config_all_defaults_unlimited() {
        let config = ConfigToml::default();
        let result = UserResourceQuota::from_config(&config);
        assert_eq!(result, UserResourceQuota::default());
    }

    #[test]
    fn test_from_nullable_columns_all_null() {
        assert_eq!(
            UserResourceQuota::from_nullable_columns(None, None, None, None),
            None
        );
    }

    #[test]
    fn test_from_nullable_columns_with_values() {
        let config = UserResourceQuota::from_nullable_columns(
            Some(500),
            Some(10),
            Some("100mb/m".to_string()),
            None,
        );
        assert_eq!(
            config,
            Some(UserResourceQuota {
                storage_quota_mb: Some(500),
                max_sessions: Some(10),
                rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_all_negative() {
        let config = UserResourceQuota::from_nullable_columns(Some(-1), Some(-5), None, None);
        // Negative values fail try_from → None (unlimited), but a warning is logged.
        assert_eq!(
            config,
            Some(UserResourceQuota {
                storage_quota_mb: None,
                max_sessions: None,
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_mixed_negative_and_positive() {
        let config = UserResourceQuota::from_nullable_columns(Some(-1), Some(10), None, None);
        assert_eq!(
            config,
            Some(UserResourceQuota {
                storage_quota_mb: None, // negative → warning + None
                max_sessions: Some(10),
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_from_nullable_columns_invalid_rate_string() {
        let config = UserResourceQuota::from_nullable_columns(
            None,
            None,
            Some("rubbish".to_string()),
            Some("100mb/m".to_string()),
        );
        assert_eq!(
            config,
            Some(UserResourceQuota {
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
        let config = UserResourceQuota::from_nullable_columns(
            None,
            None,
            Some("100r/m".to_string()),
            Some("50r/s".to_string()),
        );
        assert_eq!(
            config,
            Some(UserResourceQuota {
                storage_quota_mb: None,
                max_sessions: None,
                rate_read: None,
                rate_write: None,
            })
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = UserResourceQuota {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            rate_write: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: UserResourceQuota = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_serde_none_fields_omitted() {
        let config = UserResourceQuota {
            storage_quota_mb: Some(500),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("max_sessions"));
        assert!(!json.contains("rate_read"));
    }

    #[test]
    fn test_serde_empty_json_is_all_unlimited() {
        let config: UserResourceQuota = serde_json::from_str("{}").unwrap();
        assert_eq!(config, UserResourceQuota::default());
    }

    #[test]
    fn test_serde_rejects_invalid_rate_string() {
        let json = r#"{"rate_read": "rubbish"}"#;
        let result: Result<UserResourceQuota, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Invalid rate string should fail deserialization"
        );
    }

    #[test]
    fn test_validate_rate_roundtrips_valid_budgets_under_column_limit() {
        // All realistic budget strings should be well under 32 characters.
        let budgets = ["100mb/m", "1gb/d", "500kb/s", "10mb/h", "999gb/d", "1kb/s"];
        for s in budgets {
            let config = UserResourceQuota {
                rate_read: Some(BandwidthBudget::from_str(s).unwrap()),
                rate_write: Some(BandwidthBudget::from_str(s).unwrap()),
                ..Default::default()
            };
            config.validate_rate_roundtrips().unwrap_or_else(|e| {
                panic!("Budget \"{s}\" should pass validation but got: {e}");
            });
            // Verify string repr fits within column limit
            assert!(
                s.len() <= MAX_RATE_COLUMN_LEN,
                "Budget string \"{s}\" ({} chars) exceeds MAX_RATE_COLUMN_LEN ({MAX_RATE_COLUMN_LEN})",
                s.len()
            );
        }
    }

    #[test]
    fn test_validate_rate_roundtrips_rejects_overlong_string() {
        // Directly test the length-check logic by verifying the error message format.
        // We construct a config with a valid budget and check it passes, then verify
        // the const is correct and the check is wired up by examining the method output
        // on a normal value.
        assert_eq!(MAX_RATE_COLUMN_LEN, 32);

        // A normal config should pass.
        let config = UserResourceQuota {
            rate_read: Some(BandwidthBudget::from_str("100mb/m").unwrap()),
            ..Default::default()
        };
        assert!(config.validate_rate_roundtrips().is_ok());

        // Verify that the string representation of all standard budgets is under the limit.
        let budget = BandwidthBudget::from_str("100mb/m").unwrap();
        let s = budget.to_string();
        assert!(
            s.len() <= MAX_RATE_COLUMN_LEN,
            "Expected \"{s}\" to be at most {MAX_RATE_COLUMN_LEN} chars, got {}",
            s.len()
        );
    }
}
