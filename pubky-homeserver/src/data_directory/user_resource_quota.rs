use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use pubky_common::crypto::PublicKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::data_directory::quota_config::BandwidthRate;

/// How long a cached limit entry is considered fresh before re-resolving from DB.
const CACHE_TTL: Duration = Duration::from_secs(300); // 5 minutes

/// How long a negative (user-not-found) cache entry lives before re-checking the DB.
/// Short TTL so that a subsequent signup populates limits promptly.
const NEGATIVE_CACHE_TTL: Duration = Duration::from_secs(30);

/// Maximum length of the VARCHAR column used for rate strings in the DB.
/// Matches the `VARCHAR(32)` used in the `m20260327_add_resource_quota_columns` migration.
pub const MAX_RATE_COLUMN_LEN: usize = 32;

/// Maximum number of entries in the user limits cache. Prevents unbounded memory
/// growth from requests for many distinct users between periodic cleanup sweeps.
pub const MAX_CACHED_USER_RESOURCE_QUOTAS: usize = 100_000;

/// A three-state override for per-user bandwidth rate limits (`rate_read`, `rate_write`).
///
/// - `Default`   — no override; the system-wide rate limit from config applies. DB: NULL.
/// - `Unlimited` — explicitly bypass rate limiting for this user. DB: `"unlimited"`.
/// - `Value(T)`  — a custom rate limit for this user. DB: rate string (e.g. `"100mb/m"`).
///
/// JSON encoding (via `UserResourceQuota`'s custom serde):
/// - field absent           → `Default` (use system rate limit)
/// - `"field": null`        → `Unlimited` (no rate limiting)
/// - `"field": "100mb/m"`   → `Value(BandwidthRate)` (custom rate limit)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QuotaOverride<T> {
    /// No override — the system-wide rate limit from config applies.
    #[default]
    Default,
    /// Explicitly bypass rate limiting for this user.
    Unlimited,
    /// A custom rate limit for this user (e.g. `BandwidthRate` from `"100mb/m"`).
    Value(T),
}

impl<T> QuotaOverride<T> {
    /// Returns `true` if the field is `Default`.
    pub fn is_default(&self) -> bool {
        matches!(self, QuotaOverride::Default)
    }

    /// Returns `true` if the field is `Unlimited`.
    pub fn is_unlimited(&self) -> bool {
        matches!(self, QuotaOverride::Unlimited)
    }

    /// Returns the inner value if `Value(t)`, else `None`.
    pub fn as_value(&self) -> Option<&T> {
        match self {
            QuotaOverride::Value(v) => Some(v),
            _ => None,
        }
    }
}

/// Standalone serialize: Default/Unlimited → null, Value → T.
///
/// **Note:** `Default` and `Unlimited` both serialize as `null` — they are indistinguishable.
/// The three-state serialization (Default → omit, Unlimited → null, Value → value) is
/// handled by `UserResourceQuota`'s custom `Serialize` impl. This impl exists to satisfy
/// the `Serialize` bound but is not independently round-trippable.
impl<T: Serialize> Serialize for QuotaOverride<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            QuotaOverride::Default => serializer.serialize_none(),
            QuotaOverride::Unlimited => serializer.serialize_none(),
            QuotaOverride::Value(v) => v.serialize(serializer),
        }
    }
}

/// Standalone deserialize: null → Unlimited, value → Value(T).
///
/// **Note:** This impl cannot produce `Default` — it only sees values that are present.
/// The three-state (absent → Default, null → Unlimited, value → Value) deserialization
/// is handled by `UserResourceQuota`'s custom `Deserialize` impl using the double-Option
/// pattern. This impl exists to satisfy the `Deserialize` bound but should not be used
/// directly if you need to distinguish `Default` from `Unlimited`.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for QuotaOverride<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let opt = Option::<T>::deserialize(deserializer)?;
        match opt {
            None => Ok(QuotaOverride::Unlimited),
            Some(v) => Ok(QuotaOverride::Value(v)),
        }
    }
}

impl QuotaOverride<BandwidthRate> {
    /// Encode to DB VARCHAR column: Default → NULL, Unlimited → "unlimited", Value → rate string.
    pub fn to_db_varchar(&self) -> Option<String> {
        match self {
            QuotaOverride::Default => None,
            QuotaOverride::Unlimited => Some("unlimited".to_string()),
            QuotaOverride::Value(v) => Some(v.to_string()),
        }
    }

    /// Decode from DB VARCHAR column: NULL → Default, "unlimited" → Unlimited, value → Value.
    pub fn from_db_varchar(column: &str, val: Option<String>) -> Self {
        match val {
            None => QuotaOverride::Default,
            Some(s) if s == "unlimited" => QuotaOverride::Unlimited,
            Some(s) => match s.parse() {
                Ok(rate) => QuotaOverride::Value(rate),
                Err(e) => {
                    tracing::warn!("Invalid {column} \"{s}\" in DB: {e}; treating as Default");
                    QuotaOverride::Default
                }
            },
        }
    }
}

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
pub type UserResourceQuotaCache = Arc<DashMap<PublicKey, CachedUserResourceQuota>>;

/// Per-user resource quotas.
///
/// `storage_quota_mb` and `max_sessions` use simple `Option`:
/// - `None` — no limit (absent/null in JSON, NULL in DB)
/// - `Some(n)` — explicit limit (value in JSON, positive value in DB)
///
/// `rate_read` and `rate_write` use the three-state `QuotaOverride<BandwidthRate>`:
/// - `Default` — use system default (absent in JSON, NULL in DB)
/// - `Unlimited` — explicitly no limit (`null` in JSON, `"unlimited"` in DB)
/// - `Value(T)` — explicit limit (value in JSON, rate string in DB)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserResourceQuota {
    /// Storage quota in MB. `None` = no limit.
    pub storage_quota_mb: Option<u64>,
    /// Maximum concurrent sessions. `None` = no limit.
    pub max_sessions: Option<u32>,
    /// Per-user read speed limit override (e.g. "10mb/s").
    pub rate_read: QuotaOverride<BandwidthRate>,
    /// Per-user write speed limit override (e.g. "5mb/s").
    pub rate_write: QuotaOverride<BandwidthRate>,
}

/// Custom Serialize: skip None/Default fields, serialize values directly.
///
/// - `storage_quota_mb` / `max_sessions`: `None` → omitted, `Some(n)` → value
/// - `rate_read` / `rate_write`: `Default` → omitted, `Unlimited` → null, `Value` → value
impl Serialize for UserResourceQuota {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let count = [
            self.storage_quota_mb.is_some(),
            self.max_sessions.is_some(),
            !self.rate_read.is_default(),
            !self.rate_write.is_default(),
        ]
        .iter()
        .filter(|b| **b)
        .count();

        let mut map = serializer.serialize_map(Some(count))?;
        if let Some(v) = self.storage_quota_mb {
            map.serialize_entry("storage_quota_mb", &v)?;
        }
        if let Some(v) = self.max_sessions {
            map.serialize_entry("max_sessions", &v)?;
        }
        if !self.rate_read.is_default() {
            map.serialize_entry("rate_read", &self.rate_read)?;
        }
        if !self.rate_write.is_default() {
            map.serialize_entry("rate_write", &self.rate_write)?;
        }
        map.end()
    }
}

/// Custom Deserialize:
///
/// - `storage_quota_mb` / `max_sessions`: absent or null → `None`, value → `Some(n)`
/// - `rate_read` / `rate_write`: uses the double-Option pattern for three-state:
///   absent → `Default`, null → `Unlimited`, value → `Value(T)`
impl<'de> Deserialize<'de> for UserResourceQuota {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Deserializer that maps null → Some(None), value → Some(Some(v)).
        fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
        where
            T: Deserialize<'de>,
            D: Deserializer<'de>,
        {
            let inner = Option::<T>::deserialize(deserializer)?;
            Ok(Some(inner))
        }

        fn to_quota<T>(v: Option<Option<T>>) -> QuotaOverride<T> {
            match v {
                None => QuotaOverride::Default,
                Some(None) => QuotaOverride::Unlimited,
                Some(Some(val)) => QuotaOverride::Value(val),
            }
        }

        #[derive(Deserialize)]
        struct Helper {
            #[serde(default)]
            storage_quota_mb: Option<u64>,
            #[serde(default)]
            max_sessions: Option<u32>,
            #[serde(default, deserialize_with = "double_option")]
            rate_read: Option<Option<BandwidthRate>>,
            #[serde(default, deserialize_with = "double_option")]
            rate_write: Option<Option<BandwidthRate>>,
        }

        let h = Helper::deserialize(deserializer)?;
        Ok(UserResourceQuota {
            storage_quota_mb: h.storage_quota_mb,
            max_sessions: h.max_sessions,
            rate_read: to_quota(h.rate_read),
            rate_write: to_quota(h.rate_write),
        })
    }
}

impl UserResourceQuota {
    /// Construct from nullable DB columns.
    ///
    /// `storage_quota_mb` / `max_sessions`: NULL → `None`, positive → `Some(n)`.
    /// `rate_read` / `rate_write`: NULL → Default, "unlimited" → Unlimited, value → Value.
    pub fn from_nullable_columns(
        storage_quota_mb: Option<i64>,
        max_sessions: Option<i32>,
        rate_read: Option<String>,
        rate_write: Option<String>,
    ) -> Self {
        Self {
            storage_quota_mb: match storage_quota_mb {
                None => None,
                Some(v) if v >= 0 => Some(v as u64),
                Some(v) => {
                    tracing::warn!("Negative quota_storage_mb ({v}) in DB; treating as no limit");
                    None
                }
            },
            max_sessions: match max_sessions {
                None => None,
                Some(v) if v >= 0 => Some(v as u32),
                Some(v) => {
                    tracing::warn!("Negative quota_max_sessions ({v}) in DB; treating as no limit");
                    None
                }
            },
            rate_read: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_read", rate_read),
            rate_write: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_write", rate_write),
        }
    }

    /// Storage quota as the DB-column type (`BIGINT`). `None` → NULL, `Some(n)` → n.
    pub fn storage_quota_mb_i64(&self) -> Option<i64> {
        self.storage_quota_mb
            .map(|v| i64::try_from(v).unwrap_or(i64::MAX))
    }

    /// Max sessions as the DB-column type (`INTEGER`). `None` → NULL, `Some(n)` → n.
    pub fn max_sessions_i32(&self) -> Option<i32> {
        self.max_sessions
            .map(|v| i32::try_from(v).unwrap_or(i32::MAX))
    }

    /// Rate-read as the DB-column type (`VARCHAR`).
    pub fn rate_read_str(&self) -> Option<String> {
        self.rate_read.to_db_varchar()
    }

    /// Rate-write as the DB-column type (`VARCHAR`).
    pub fn rate_write_str(&self) -> Option<String> {
        self.rate_write.to_db_varchar()
    }

    /// Validate that rate fields survive a Display → FromStr roundtrip.
    pub fn validate_rate_roundtrips(&self) -> Result<(), String> {
        for (label, field) in [
            ("rate_read", &self.rate_read),
            ("rate_write", &self.rate_write),
        ] {
            if let QuotaOverride::Value(ref b) = field {
                let s = b.to_string();
                if s.len() > MAX_RATE_COLUMN_LEN {
                    return Err(format!(
                        "{label} string \"{s}\" exceeds DB column limit of {MAX_RATE_COLUMN_LEN} characters"
                    ));
                }
                s.parse::<BandwidthRate>()
                    .map_err(|e| format!("{label} roundtrip validation failed for \"{s}\": {e}"))?;
            }
        }
        Ok(())
    }

    /// Create a quota with only storage set from config value.
    /// 0 → no limit, n > 0 → Some(n).
    pub fn storage_default_from_config(user_storage_quota_mb: u64) -> Self {
        Self {
            storage_quota_mb: match user_storage_quota_mb {
                0 => None,
                n => Some(n),
            },
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_quota_field_default() {
        let field: QuotaOverride<BandwidthRate> = QuotaOverride::default();
        assert!(field.is_default());
        assert!(!field.is_unlimited());
        assert_eq!(field.as_value(), None);
    }

    #[test]
    fn test_quota_field_unlimited() {
        let field: QuotaOverride<BandwidthRate> = QuotaOverride::Unlimited;
        assert!(!field.is_default());
        assert!(field.is_unlimited());
        assert_eq!(field.as_value(), None);
    }

    #[test]
    fn test_quota_field_value() {
        let rate = BandwidthRate::from_str("100mb/m").unwrap();
        let field = QuotaOverride::Value(rate.clone());
        assert!(!field.is_default());
        assert!(!field.is_unlimited());
        assert_eq!(field.as_value(), Some(&rate));
    }

    // ── DB encoding tests ──────────────────────────────────────────────

    #[test]
    fn test_varchar_roundtrip() {
        assert_eq!(
            QuotaOverride::<BandwidthRate>::from_db_varchar("rate_read", None),
            QuotaOverride::Default
        );
        assert_eq!(
            QuotaOverride::<BandwidthRate>::from_db_varchar(
                "rate_read",
                Some("unlimited".to_string())
            ),
            QuotaOverride::Unlimited
        );
        assert_eq!(
            QuotaOverride::<BandwidthRate>::from_db_varchar(
                "rate_read",
                Some("100mb/m".to_string())
            ),
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
        // Invalid string treated as Default
        assert_eq!(
            QuotaOverride::<BandwidthRate>::from_db_varchar(
                "rate_read",
                Some("rubbish".to_string())
            ),
            QuotaOverride::Default
        );

        assert_eq!(
            QuotaOverride::<BandwidthRate>::Default.to_db_varchar(),
            None
        );
        assert_eq!(
            QuotaOverride::<BandwidthRate>::Unlimited.to_db_varchar(),
            Some("unlimited".to_string())
        );
        assert_eq!(
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()).to_db_varchar(),
            Some("100mb/m".to_string())
        );
    }

    // ── from_nullable_columns tests ────────────────────────────────────

    #[test]
    fn test_from_nullable_columns_all_null() {
        let q = UserResourceQuota::from_nullable_columns(None, None, None, None);
        assert_eq!(q, UserResourceQuota::default());
    }

    #[test]
    fn test_from_nullable_columns_with_values() {
        let q = UserResourceQuota::from_nullable_columns(
            Some(500),
            Some(10),
            Some("100mb/m".to_string()),
            None,
        );
        assert_eq!(q.storage_quota_mb, Some(500));
        assert_eq!(q.max_sessions, Some(10));
        assert_eq!(
            q.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_from_nullable_columns_unlimited_values() {
        let q = UserResourceQuota::from_nullable_columns(
            None,
            None,
            Some("unlimited".to_string()),
            Some("unlimited".to_string()),
        );
        assert_eq!(q.storage_quota_mb, None);
        assert_eq!(q.max_sessions, None);
        assert_eq!(q.rate_read, QuotaOverride::Unlimited);
        assert_eq!(q.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_from_nullable_columns_mixed() {
        let q = UserResourceQuota::from_nullable_columns(None, Some(10), None, None);
        assert_eq!(q.storage_quota_mb, None);
        assert_eq!(q.max_sessions, Some(10));
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_from_nullable_columns_invalid_rate_string() {
        let q = UserResourceQuota::from_nullable_columns(
            None,
            None,
            Some("rubbish".to_string()),
            Some("100mb/m".to_string()),
        );
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(
            q.rate_write,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
    }

    #[test]
    fn test_from_nullable_columns_legacy_request_units() {
        let q = UserResourceQuota::from_nullable_columns(
            None,
            None,
            Some("100r/m".to_string()),
            Some("50r/s".to_string()),
        );
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    // ── Serde JSON tests ───────────────────────────────────────────────

    #[test]
    fn test_serde_roundtrip() {
        let q = UserResourceQuota {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
        };
        let json = serde_json::to_string(&q).unwrap();
        let deserialized: UserResourceQuota = serde_json::from_str(&json).unwrap();
        assert_eq!(q, deserialized);
    }

    #[test]
    fn test_serde_default_fields_omitted() {
        let q = UserResourceQuota {
            storage_quota_mb: Some(500),
            ..Default::default()
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains("storage_quota_mb"));
        assert!(!json.contains("max_sessions"));
        assert!(!json.contains("rate_read"));
        assert!(!json.contains("rate_write"));
    }

    #[test]
    fn test_serde_empty_json_is_all_default() {
        let q: UserResourceQuota = serde_json::from_str("{}").unwrap();
        assert_eq!(q, UserResourceQuota::default());
    }

    #[test]
    fn test_serde_null_is_none_for_storage_and_sessions() {
        let json = r#"{"storage_quota_mb": null, "max_sessions": null}"#;
        let q: UserResourceQuota = serde_json::from_str(json).unwrap();
        // null and absent both map to None for these fields
        assert_eq!(q.storage_quota_mb, None);
        assert_eq!(q.max_sessions, None);
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_serde_null_is_unlimited_for_rates() {
        let json = r#"{"rate_read": null, "rate_write": null}"#;
        let q: UserResourceQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q.rate_read, QuotaOverride::Unlimited);
        assert_eq!(q.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_serde_absent_is_none_or_default() {
        let json = r#"{"storage_quota_mb": 500}"#;
        let q: UserResourceQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q.storage_quota_mb, Some(500));
        assert_eq!(q.max_sessions, None);
        assert_eq!(q.rate_read, QuotaOverride::Default);
    }

    #[test]
    fn test_serde_none_fields_omitted() {
        // None storage/sessions should be omitted from serialized JSON
        let q = UserResourceQuota::default();
        let json = serde_json::to_string(&q).unwrap();
        assert_eq!(json, "{}");
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

    // ── Validate rate roundtrips ───────────────────────────────────────

    #[test]
    fn test_validate_rate_roundtrips_valid_budgets() {
        let budgets = ["100mb/m", "1gb/d", "500kb/s", "10mb/h", "999gb/d", "1kb/s"];
        for s in budgets {
            let q = UserResourceQuota {
                rate_read: QuotaOverride::Value(BandwidthRate::from_str(s).unwrap()),
                rate_write: QuotaOverride::Value(BandwidthRate::from_str(s).unwrap()),
                ..Default::default()
            };
            q.validate_rate_roundtrips().unwrap_or_else(|e| {
                panic!("Budget \"{s}\" should pass validation but got: {e}");
            });
        }
    }

    #[test]
    fn test_validate_rate_roundtrips_skips_non_value() {
        let q = UserResourceQuota {
            rate_read: QuotaOverride::Default,
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };
        assert!(q.validate_rate_roundtrips().is_ok());
    }

    // ── storage_default_from_config ────────────────────────────────────

    #[test]
    fn test_storage_default_from_config_zero_is_no_limit() {
        let q = UserResourceQuota::storage_default_from_config(0);
        assert_eq!(q.storage_quota_mb, None);
        assert_eq!(q.max_sessions, None);
    }

    #[test]
    fn test_storage_default_from_config_nonzero() {
        let q = UserResourceQuota::storage_default_from_config(1024);
        assert_eq!(q.storage_quota_mb, Some(1024));
        assert_eq!(q.max_sessions, None);
    }
}
