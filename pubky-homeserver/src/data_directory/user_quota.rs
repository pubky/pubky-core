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
/// Matches the `VARCHAR(32)` used in the `m20260327_add_quota_columns` migration.
pub const MAX_RATE_COLUMN_LEN: usize = 32;

/// Maximum number of entries in the user limits cache. Prevents unbounded memory
/// growth from requests for many distinct users between periodic cleanup sweeps.
pub const MAX_CACHED_USER_QUOTAS: usize = 100_000;

/// A three-state override for per-user quota fields.
///
/// Semantics:
/// - `Default`   — no override; the system-wide default applies.
/// - `Unlimited` — explicitly bypass the limit for this user.
/// - `Value(T)`  — a custom limit for this user.
///
/// ## JSON encoding
///
/// | JSON | Variant |
/// |---|---|
/// | field absent | `Default` (use system default) |
/// | `null` | `Default` (use system default) |
/// | `"unlimited"` | `Unlimited` (no limit) |
/// | value | `Value(T)` (custom limit) |
///
/// ## DB encoding
///
/// | Variant | Integer columns | VARCHAR columns |
/// |---|---|---|
/// | `Default` | `NULL` | `NULL` |
/// | `Unlimited` | `-1` | `"unlimited"` |
/// | `Value(T)` | positive value | rate string |
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum QuotaOverride<T> {
    /// No override — the system-wide default applies.
    #[default]
    Default,
    /// Explicitly bypass the limit for this user.
    Unlimited,
    /// A custom limit for this user.
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

/// Serialize: Default → null, Unlimited → "unlimited", Value → T.
///
/// In the context of `UserQuota`, `Default` fields are skipped entirely
/// (via `skip_serializing_if`), so the null serialization of `Default`
/// only appears if `QuotaOverride` is serialized standalone.
impl<T: Serialize> Serialize for QuotaOverride<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            QuotaOverride::Default => serializer.serialize_none(),
            QuotaOverride::Unlimited => serializer.serialize_str("unlimited"),
            QuotaOverride::Value(v) => v.serialize(serializer),
        }
    }
}

/// Deserialize: null → Default, "unlimited" → Unlimited, value → Value(T).
///
/// Uses `serde_json::Value` as an intermediate to distinguish between
/// null, the string "unlimited", and other values.
impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for QuotaOverride<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match &value {
            serde_json::Value::Null => Ok(QuotaOverride::Default),
            serde_json::Value::String(s) if s == "unlimited" => Ok(QuotaOverride::Unlimited),
            _ => serde_json::from_value(value)
                .map(QuotaOverride::Value)
                .map_err(serde::de::Error::custom),
        }
    }
}

impl QuotaOverride<u64> {
    /// Encode to DB BIGINT column: Default → NULL, Unlimited → -1, Value → positive.
    pub fn to_db_bigint(&self) -> Option<i64> {
        match self {
            QuotaOverride::Default => None,
            QuotaOverride::Unlimited => Some(-1),
            QuotaOverride::Value(v) => Some(i64::try_from(*v).unwrap_or(i64::MAX)),
        }
    }

    /// Decode from DB BIGINT column: NULL → Default, -1 → Unlimited, positive → Value.
    pub fn from_db_bigint(column: &str, val: Option<i64>) -> Self {
        match val {
            None => QuotaOverride::Default,
            Some(-1) => QuotaOverride::Unlimited,
            Some(v) if v >= 0 => QuotaOverride::Value(v as u64),
            Some(v) => {
                tracing::warn!("Unexpected {column} ({v}) in DB; treating as Default");
                QuotaOverride::Default
            }
        }
    }

    /// Resolve to an effective `Option<u64>` using a system default.
    ///
    /// - `Default` → `system_default`
    /// - `Unlimited` → `None`
    /// - `Value(v)` → `Some(v)`
    pub fn resolve_with_default(&self, system_default: Option<u64>) -> Option<u64> {
        match self {
            QuotaOverride::Default => system_default,
            QuotaOverride::Unlimited => None,
            QuotaOverride::Value(v) => Some(*v),
        }
    }
}

impl QuotaOverride<u32> {
    /// Encode to DB INTEGER column: Default → NULL, Unlimited → -1, Value → positive.
    pub fn to_db_int(&self) -> Option<i32> {
        match self {
            QuotaOverride::Default => None,
            QuotaOverride::Unlimited => Some(-1),
            QuotaOverride::Value(v) => Some(i32::try_from(*v).unwrap_or(i32::MAX)),
        }
    }

    /// Decode from DB INTEGER column: NULL → Default, -1 → Unlimited, positive → Value.
    pub fn from_db_int(column: &str, val: Option<i32>) -> Self {
        match val {
            None => QuotaOverride::Default,
            Some(-1) => QuotaOverride::Unlimited,
            Some(v) if v >= 0 => QuotaOverride::Value(v as u32),
            Some(v) => {
                tracing::warn!("Unexpected {column} ({v}) in DB; treating as Default");
                QuotaOverride::Default
            }
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
pub struct CachedUserQuota {
    /// The resolved limit configuration, or `None` for a negative (user-not-found) entry.
    pub config: Option<UserQuota>,
    cached_at: Instant,
    ttl: Duration,
}

impl CachedUserQuota {
    /// Wrap a resolved config with a fresh timestamp.
    pub fn new(config: UserQuota) -> Self {
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
pub type UserQuotaCache = Arc<DashMap<PublicKey, CachedUserQuota>>;

/// Per-user quotas. All fields use the three-state `QuotaOverride<T>`.
///
/// | Field | Default means | Example Value |
/// |---|---|---|
/// | `storage_quota_mb` | use `user_storage_quota_mb` from config | `Value(500)` = 500 MB |
/// | `max_sessions` | unlimited (no built-in cap) | `Value(10)` = 10 sessions |
/// | `rate_read` | use path-based rate limit from config | `Value(BandwidthRate)` |
/// | `rate_write` | use path-based rate limit from config | `Value(BandwidthRate)` |
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserQuota {
    /// Storage quota in MB.
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub storage_quota_mb: QuotaOverride<u64>,
    /// Maximum concurrent sessions.
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub max_sessions: QuotaOverride<u32>,
    /// Per-user read speed limit override (e.g. "10mb/s").
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub rate_read: QuotaOverride<BandwidthRate>,
    /// Per-user write speed limit override (e.g. "5mb/s").
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub rate_write: QuotaOverride<BandwidthRate>,
}

impl UserQuota {
    /// Construct from nullable DB columns.
    ///
    /// - Integer columns: NULL → Default, -1 → Unlimited, positive → Value.
    /// - VARCHAR columns: NULL → Default, "unlimited" → Unlimited, value → Value.
    pub fn from_nullable_columns(
        storage_quota_mb: Option<i64>,
        max_sessions: Option<i32>,
        rate_read: Option<String>,
        rate_write: Option<String>,
    ) -> Self {
        Self {
            storage_quota_mb: QuotaOverride::<u64>::from_db_bigint(
                "quota_storage_mb",
                storage_quota_mb,
            ),
            max_sessions: QuotaOverride::<u32>::from_db_int("quota_max_sessions", max_sessions),
            rate_read: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_read", rate_read),
            rate_write: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_write", rate_write),
        }
    }

    /// Storage quota as the DB-column type (`BIGINT`).
    pub fn storage_quota_mb_i64(&self) -> Option<i64> {
        self.storage_quota_mb.to_db_bigint()
    }

    /// Max sessions as the DB-column type (`INTEGER`).
    pub fn max_sessions_i32(&self) -> Option<i32> {
        self.max_sessions.to_db_int()
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

    /// Merge a patch into this quota: only `Some` fields are updated; `None` means keep.
    pub fn merge(&mut self, patch: &UserQuotaPatch) {
        if let Some(ref v) = patch.storage_quota_mb {
            self.storage_quota_mb = v.clone();
        }
        if let Some(ref v) = patch.max_sessions {
            self.max_sessions = v.clone();
        }
        if let Some(ref v) = patch.rate_read {
            self.rate_read = v.clone();
        }
        if let Some(ref v) = patch.rate_write {
            self.rate_write = v.clone();
        }
    }
}

/// Serde helper: when the field is present, delegates to `QuotaOverride::deserialize`
/// which maps null → `Default`, "unlimited" → `Unlimited`, value → `Value(T)`.
/// When absent, `#[serde(default)]` gives `None` (keep unchanged).
fn deserialize_patch_override<'de, T, D>(d: D) -> Result<Option<QuotaOverride<T>>, D::Error>
where
    T: serde::de::DeserializeOwned,
    D: Deserializer<'de>,
{
    QuotaOverride::<T>::deserialize(d).map(Some)
}

/// Partial update for `UserQuota`.
///
/// Used by the PATCH endpoint: only fields present in the JSON body are
/// applied to the existing quota. Absent fields are left unchanged.
///
/// | JSON | Effect |
/// |---|---|
/// | field absent | keep existing value |
/// | `null` | reset to `Default` (use system default) |
/// | `"unlimited"` | set to `Unlimited` (no limit) |
/// | value | set to `Value(v)` (custom limit) |
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserQuotaPatch {
    /// Storage quota in MB.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub storage_quota_mb: Option<QuotaOverride<u64>>,
    /// Maximum concurrent sessions.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub max_sessions: Option<QuotaOverride<u32>>,
    /// Per-user read rate limit.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub rate_read: Option<QuotaOverride<BandwidthRate>>,
    /// Per-user write rate limit.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub rate_write: Option<QuotaOverride<BandwidthRate>>,
}

impl UserQuotaPatch {
    /// Validate that any rate fields with values survive a roundtrip.
    pub fn validate_rate_roundtrips(&self) -> Result<(), String> {
        for (label, field) in [
            ("rate_read", &self.rate_read),
            ("rate_write", &self.rate_write),
        ] {
            if let Some(QuotaOverride::Value(ref b)) = field {
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

    #[test]
    fn test_bigint_roundtrip() {
        assert_eq!(
            QuotaOverride::<u64>::from_db_bigint("quota_storage_mb", None),
            QuotaOverride::Default
        );
        assert_eq!(
            QuotaOverride::<u64>::from_db_bigint("quota_storage_mb", Some(-1)),
            QuotaOverride::Unlimited
        );
        assert_eq!(
            QuotaOverride::<u64>::from_db_bigint("quota_storage_mb", Some(500)),
            QuotaOverride::Value(500)
        );
        assert_eq!(
            QuotaOverride::<u64>::from_db_bigint("quota_storage_mb", Some(0)),
            QuotaOverride::Value(0)
        );
        assert_eq!(
            QuotaOverride::<u64>::from_db_bigint("quota_storage_mb", Some(-5)),
            QuotaOverride::Default
        );

        assert_eq!(QuotaOverride::<u64>::Default.to_db_bigint(), None);
        assert_eq!(QuotaOverride::<u64>::Unlimited.to_db_bigint(), Some(-1));
        assert_eq!(QuotaOverride::Value(500u64).to_db_bigint(), Some(500));
    }

    #[test]
    fn test_int_roundtrip() {
        assert_eq!(
            QuotaOverride::<u32>::from_db_int("quota_max_sessions", None),
            QuotaOverride::Default
        );
        assert_eq!(
            QuotaOverride::<u32>::from_db_int("quota_max_sessions", Some(-1)),
            QuotaOverride::Unlimited
        );
        assert_eq!(
            QuotaOverride::<u32>::from_db_int("quota_max_sessions", Some(10)),
            QuotaOverride::Value(10)
        );
        assert_eq!(QuotaOverride::<u32>::Default.to_db_int(), None);
        assert_eq!(QuotaOverride::<u32>::Unlimited.to_db_int(), Some(-1));
        assert_eq!(QuotaOverride::Value(10u32).to_db_int(), Some(10));
    }

    #[test]
    fn test_resolve_with_default() {
        assert_eq!(
            QuotaOverride::<u64>::Default.resolve_with_default(Some(500)),
            Some(500)
        );
        assert_eq!(
            QuotaOverride::<u64>::Default.resolve_with_default(None),
            None
        );
        assert_eq!(
            QuotaOverride::<u64>::Unlimited.resolve_with_default(Some(500)),
            None
        );
        assert_eq!(
            QuotaOverride::Value(200u64).resolve_with_default(Some(500)),
            Some(200)
        );
    }

    #[test]
    fn test_from_nullable_columns_all_null() {
        let q = UserQuota::from_nullable_columns(None, None, None, None);
        assert_eq!(q, UserQuota::default());
    }

    #[test]
    fn test_from_nullable_columns_with_values() {
        let q = UserQuota::from_nullable_columns(
            Some(500),
            Some(10),
            Some("100mb/m".to_string()),
            None,
        );
        assert_eq!(q.storage_quota_mb, QuotaOverride::Value(500));
        assert_eq!(q.max_sessions, QuotaOverride::Value(10));
        assert_eq!(
            q.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_from_nullable_columns_unlimited_values() {
        let q = UserQuota::from_nullable_columns(
            Some(-1),
            Some(-1),
            Some("unlimited".to_string()),
            Some("unlimited".to_string()),
        );
        assert_eq!(q.storage_quota_mb, QuotaOverride::Unlimited);
        assert_eq!(q.max_sessions, QuotaOverride::Unlimited);
        assert_eq!(q.rate_read, QuotaOverride::Unlimited);
        assert_eq!(q.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_from_nullable_columns_mixed() {
        let q = UserQuota::from_nullable_columns(None, Some(10), None, None);
        assert_eq!(q.storage_quota_mb, QuotaOverride::Default);
        assert_eq!(q.max_sessions, QuotaOverride::Value(10));
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_from_nullable_columns_invalid_rate_string() {
        let q = UserQuota::from_nullable_columns(
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
        let q = UserQuota::from_nullable_columns(
            None,
            None,
            Some("100r/m".to_string()),
            Some("50r/s".to_string()),
        );
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_serde_roundtrip() {
        let q = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            max_sessions: QuotaOverride::Value(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
        };
        let json = serde_json::to_string(&q).unwrap();
        let deserialized: UserQuota = serde_json::from_str(&json).unwrap();
        assert_eq!(q, deserialized);
    }

    #[test]
    fn test_serde_default_fields_omitted() {
        let q = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
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
        let q: UserQuota = serde_json::from_str("{}").unwrap();
        assert_eq!(q, UserQuota::default());
    }

    #[test]
    fn test_serde_null_is_default_for_all() {
        let json = r#"{"storage_quota_mb": null, "max_sessions": null, "rate_read": null, "rate_write": null}"#;
        let q: UserQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q, UserQuota::default());
    }

    #[test]
    fn test_serde_unlimited_string() {
        let json = r#"{"storage_quota_mb": "unlimited", "max_sessions": "unlimited", "rate_read": "unlimited", "rate_write": "unlimited"}"#;
        let q: UserQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q.storage_quota_mb, QuotaOverride::Unlimited);
        assert_eq!(q.max_sessions, QuotaOverride::Unlimited);
        assert_eq!(q.rate_read, QuotaOverride::Unlimited);
        assert_eq!(q.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_serde_unlimited_serializes_as_string() {
        let q = UserQuota {
            storage_quota_mb: QuotaOverride::Unlimited,
            rate_read: QuotaOverride::Unlimited,
            ..Default::default()
        };
        let json = serde_json::to_string(&q).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["storage_quota_mb"], "unlimited");
        assert_eq!(v["rate_read"], "unlimited");
    }

    #[test]
    fn test_serde_absent_is_default() {
        let json = r#"{"storage_quota_mb": 500}"#;
        let q: UserQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q.storage_quota_mb, QuotaOverride::Value(500));
        assert_eq!(q.max_sessions, QuotaOverride::Default);
        assert_eq!(q.rate_read, QuotaOverride::Default);
    }

    #[test]
    fn test_serde_none_fields_omitted() {
        let q = UserQuota::default();
        let json = serde_json::to_string(&q).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn test_serde_rejects_invalid_rate_string() {
        let json = r#"{"rate_read": "rubbish"}"#;
        let result: Result<UserQuota, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "Invalid rate string should fail deserialization"
        );
    }

    #[test]
    fn test_validate_rate_roundtrips_valid_budgets() {
        let budgets = ["100mb/m", "1gb/d", "500kb/s", "10mb/h", "999gb/d", "1kb/s"];
        for s in budgets {
            let q = UserQuota {
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
        let q = UserQuota {
            rate_read: QuotaOverride::Default,
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };
        assert!(q.validate_rate_roundtrips().is_ok());
    }

    // ── Patch tests ──

    #[test]
    fn test_patch_empty_body_changes_nothing() {
        let patch: UserQuotaPatch = serde_json::from_str("{}").unwrap();
        assert!(patch.storage_quota_mb.is_none());
        assert!(patch.max_sessions.is_none());
        assert!(patch.rate_read.is_none());
        assert!(patch.rate_write.is_none());
    }

    #[test]
    fn test_patch_null_resets_to_default() {
        let json = r#"{"rate_read": null, "storage_quota_mb": null}"#;
        let patch: UserQuotaPatch = serde_json::from_str(json).unwrap();
        assert_eq!(patch.storage_quota_mb, Some(QuotaOverride::Default));
        assert_eq!(patch.rate_read, Some(QuotaOverride::Default));
        // absent → None (keep)
        assert!(patch.max_sessions.is_none());
        assert!(patch.rate_write.is_none());
    }

    #[test]
    fn test_patch_unlimited_string() {
        let json = r#"{"storage_quota_mb": "unlimited", "rate_write": "unlimited"}"#;
        let patch: UserQuotaPatch = serde_json::from_str(json).unwrap();
        assert_eq!(patch.storage_quota_mb, Some(QuotaOverride::Unlimited));
        assert_eq!(patch.rate_write, Some(QuotaOverride::Unlimited));
    }

    #[test]
    fn test_patch_value_sets_value() {
        let json = r#"{"storage_quota_mb": 500, "rate_write": "10mb/s"}"#;
        let patch: UserQuotaPatch = serde_json::from_str(json).unwrap();
        assert_eq!(patch.storage_quota_mb, Some(QuotaOverride::Value(500)));
        assert_eq!(
            patch.rate_write,
            Some(QuotaOverride::Value(
                BandwidthRate::from_str("10mb/s").unwrap()
            ))
        );
        assert!(patch.max_sessions.is_none());
        assert!(patch.rate_read.is_none());
    }

    #[test]
    fn test_merge_applies_only_present_fields() {
        let mut base = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            max_sessions: QuotaOverride::Value(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Value(BandwidthRate::from_str("50mb/s").unwrap()),
        };

        // Patch only storage_quota_mb and rate_write; others should be unchanged
        let patch: UserQuotaPatch =
            serde_json::from_str(r#"{"storage_quota_mb": 200, "rate_write": "unlimited"}"#)
                .unwrap();
        base.merge(&patch);

        assert_eq!(base.storage_quota_mb, QuotaOverride::Value(200));
        assert_eq!(base.max_sessions, QuotaOverride::Value(10)); // unchanged
        assert_eq!(
            base.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        ); // unchanged
        assert_eq!(base.rate_write, QuotaOverride::Unlimited); // patched
    }

    #[test]
    fn test_merge_null_resets_to_default() {
        let mut base = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            ..Default::default()
        };

        let patch: UserQuotaPatch =
            serde_json::from_str(r#"{"storage_quota_mb": null, "rate_read": null}"#).unwrap();
        base.merge(&patch);

        assert_eq!(base.storage_quota_mb, QuotaOverride::Default);
        assert_eq!(base.rate_read, QuotaOverride::Default);
    }

    #[test]
    fn test_merge_empty_patch_is_noop() {
        let original = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            max_sessions: QuotaOverride::Value(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
        };
        let mut patched = original.clone();
        let patch: UserQuotaPatch = serde_json::from_str("{}").unwrap();
        patched.merge(&patch);
        assert_eq!(patched, original);
    }
}
