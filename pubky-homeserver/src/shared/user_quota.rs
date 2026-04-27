//! Per-user quota domain types.
//!
//! These types model the per-user overrides for storage and bandwidth limits.
//! They are shared across the codebase: persistence entities use them to
//! convert raw DB columns into typed values, the service layer uses them for
//! enforcement and caching, and route handlers use them for API serialization.
//!
//! Key types:
//! - [`QuotaOverride<T>`] — three-state enum: Default / Unlimited / Value(T).
//! - [`UserQuota`] — the full set of per-user quota fields.
//! - [`UserQuotaPatch`] — partial update for PATCH semantics (absent = keep).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::data_directory::quota_config::BandwidthRate;
use crate::data_directory::DefaultQuotasToml;

/// Maximum length of the VARCHAR column used for rate strings in the DB.
/// Matches the `VARCHAR(32)` used in the `m20260327_add_quota_columns` migration.
pub const MAX_RATE_COLUMN_LEN: usize = 32;

/// Sentinel value stored in BIGINT columns to represent "Unlimited".
const DB_UNLIMITED_SENTINEL: i64 = -1;

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
    #[cfg(test)]
    pub fn is_unlimited(&self) -> bool {
        matches!(self, QuotaOverride::Unlimited)
    }

    /// Returns the inner value if `Value(t)`, else `None`.
    #[cfg(test)]
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
            QuotaOverride::Unlimited => Some(DB_UNLIMITED_SENTINEL),
            QuotaOverride::Value(v) => Some(i64::try_from(*v).unwrap_or(i64::MAX)),
        }
    }

    /// Decode from DB BIGINT column: NULL → Default, -1 → Unlimited, positive → Value.
    pub fn from_db_bigint(column: &str, val: Option<i64>) -> Self {
        match val {
            None => QuotaOverride::Default,
            Some(DB_UNLIMITED_SENTINEL) => QuotaOverride::Unlimited,
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

impl QuotaOverride<BandwidthRate> {
    /// Resolve to an effective `Option<BandwidthRate>` using a system default.
    ///
    /// - `Default`   → `system_default`
    /// - `Unlimited` → `None`
    /// - `Value(v)`  → `Some(v)`
    pub fn resolve_with_default(
        &self,
        system_default: Option<&BandwidthRate>,
    ) -> Option<BandwidthRate> {
        match self {
            QuotaOverride::Default => system_default.cloned(),
            QuotaOverride::Unlimited => None,
            QuotaOverride::Value(v) => Some(v.clone()),
        }
    }

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

/// Convert an `Option<u32>` burst value to the DB column type (`INTEGER`),
/// truncating to `i32::MAX` with a warning if the value overflows.
fn burst_to_i32(label: &str, value: Option<u32>) -> Option<i32> {
    value.map(|v| {
        i32::try_from(v).unwrap_or_else(|_| {
            tracing::warn!("{label} {v} exceeds i32::MAX, truncating to {}", i32::MAX);
            i32::MAX
        })
    })
}

/// Validate that a `BandwidthRate` value can be persisted: its string form
/// must fit the DB column and parse back to the same rate.
fn validate_rate_value(label: &str, field: &QuotaOverride<BandwidthRate>) -> Result<(), String> {
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
    Ok(())
}

/// Validate that a burst value (if present) is > 0 and fits in the DB column (i32).
fn validate_burst_value(label: &str, burst: Option<u32>) -> Result<(), String> {
    if let Some(b) = burst {
        if b == 0 {
            return Err(format!("{label} must be greater than 0"));
        }
        if b > i32::MAX as u32 {
            return Err(format!("{label} value {b} exceeds maximum ({})", i32::MAX));
        }
    }
    Ok(())
}

/// Validate a burst field that also requires a corresponding rate `Value`.
fn validate_burst(
    label: &str,
    burst: Option<u32>,
    rate: &QuotaOverride<BandwidthRate>,
) -> Result<(), String> {
    if burst.is_some() && !matches!(rate, QuotaOverride::Value(_)) {
        return Err(format!(
            "{label} requires the corresponding rate to be set to a value"
        ));
    }
    validate_burst_value(label, burst)
}

/// Per-user quotas. All fields use the three-state `QuotaOverride<T>`.
///
/// | Field | Default means | Example Value |
/// |---|---|---|
/// | `storage_quota_mb` | use `storage.default_quota_mb` from config | `Value(500)` = 500 MB |
/// | `rate_read` | use `default_quotas.rate_read` from config | `Value(BandwidthRate)` |
/// | `rate_write` | use `default_quotas.rate_write` from config | `Value(BandwidthRate)` |
/// | `rate_read_burst` | burst = rate | `Some(50)` = 50 in rate's unit |
/// | `rate_write_burst` | burst = rate | `Some(50)` = 50 in rate's unit |
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserQuota {
    /// Storage quota in MB.
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub storage_quota_mb: QuotaOverride<u64>,
    /// Per-user read speed limit override (e.g. "10mb/s").
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub rate_read: QuotaOverride<BandwidthRate>,
    /// Per-user write speed limit override (e.g. "5mb/s").
    #[serde(default, skip_serializing_if = "QuotaOverride::is_default")]
    pub rate_write: QuotaOverride<BandwidthRate>,
    /// Burst override for read speed limit, in the rate's natural unit
    /// (MB for "…mb/s", KB for "…kb/s"). `None` = burst equals rate (default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_read_burst: Option<u32>,
    /// Burst override for write speed limit, in the rate's natural unit
    /// (MB for "…mb/s", KB for "…kb/s"). `None` = burst equals rate (default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_write_burst: Option<u32>,
}

impl UserQuota {
    /// Construct from nullable DB columns.
    ///
    /// - Integer columns: NULL → Default, -1 → Unlimited, positive → Value.
    /// - VARCHAR columns: NULL → Default, "unlimited" → Unlimited, value → Value.
    pub fn from_nullable_columns(
        storage_quota_mb: Option<i64>,
        rate_read: Option<String>,
        rate_write: Option<String>,
        rate_read_burst: Option<i32>,
        rate_write_burst: Option<i32>,
    ) -> Self {
        Self {
            storage_quota_mb: QuotaOverride::<u64>::from_db_bigint(
                "quota_storage_mb",
                storage_quota_mb,
            ),
            rate_read: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_read", rate_read),
            rate_write: QuotaOverride::<BandwidthRate>::from_db_varchar("rate_write", rate_write),
            rate_read_burst: rate_read_burst.and_then(|v| u32::try_from(v).ok()),
            rate_write_burst: rate_write_burst.and_then(|v| u32::try_from(v).ok()),
        }
    }

    /// Storage quota as the DB-column type (`BIGINT`).
    pub fn storage_quota_mb_i64(&self) -> Option<i64> {
        self.storage_quota_mb.to_db_bigint()
    }

    /// Rate-read as the DB-column type (`VARCHAR`).
    pub fn rate_read_str(&self) -> Option<String> {
        self.rate_read.to_db_varchar()
    }

    /// Rate-write as the DB-column type (`VARCHAR`).
    pub fn rate_write_str(&self) -> Option<String> {
        self.rate_write.to_db_varchar()
    }

    /// Rate-read burst as DB-column type (`INTEGER`).
    pub fn rate_read_burst_i32(&self) -> Option<i32> {
        burst_to_i32("rate_read_burst", self.rate_read_burst)
    }

    /// Rate-write burst as DB-column type (`INTEGER`).
    pub fn rate_write_burst_i32(&self) -> Option<i32> {
        burst_to_i32("rate_write_burst", self.rate_write_burst)
    }

    /// Resolve all `Default` fields against system-wide defaults.
    ///
    /// After resolution every field is either `Value(…)` or `Unlimited`,
    /// so `skip_serializing_if = "is_default"` will *not* omit any field —
    /// the caller gets the full effective quota in the JSON response.
    pub fn resolve_with_defaults(
        &self,
        default_storage_mb: Option<u64>,
        default_quotas: &DefaultQuotasToml,
    ) -> Self {
        fn resolve_u64(field: &QuotaOverride<u64>, default: Option<u64>) -> QuotaOverride<u64> {
            match field {
                QuotaOverride::Default => match default {
                    Some(v) => QuotaOverride::Value(v),
                    None => QuotaOverride::Unlimited,
                },
                other => other.clone(),
            }
        }
        fn resolve_bw(
            field: &QuotaOverride<BandwidthRate>,
            default: Option<&BandwidthRate>,
        ) -> QuotaOverride<BandwidthRate> {
            match field {
                QuotaOverride::Default => match default {
                    Some(v) => QuotaOverride::Value(v.clone()),
                    None => QuotaOverride::Unlimited,
                },
                other => other.clone(),
            }
        }

        Self {
            storage_quota_mb: resolve_u64(&self.storage_quota_mb, default_storage_mb),
            rate_read: resolve_bw(&self.rate_read, default_quotas.rate_read.as_ref()),
            rate_write: resolve_bw(&self.rate_write, default_quotas.rate_write.as_ref()),
            rate_read_burst: self.rate_read_burst.or(if self.rate_read.is_default() {
                default_quotas.rate_read_burst
            } else {
                None
            }),
            rate_write_burst: self.rate_write_burst.or(if self.rate_write.is_default() {
                default_quotas.rate_write_burst
            } else {
                None
            }),
        }
    }

    /// Check that the quota fields are internally consistent:
    /// - Rate values can be persisted (fit in the DB column).
    /// - Burst overrides have a corresponding rate `Value`.
    /// - Burst values are > 0.
    pub fn validate(&self) -> Result<(), String> {
        validate_rate_value("rate_read", &self.rate_read)?;
        validate_rate_value("rate_write", &self.rate_write)?;
        validate_burst("rate_read_burst", self.rate_read_burst, &self.rate_read)?;
        validate_burst("rate_write_burst", self.rate_write_burst, &self.rate_write)?;
        Ok(())
    }

    /// Merge a patch into this quota: only `Some` fields are updated; `None` means keep.
    pub fn merge(&mut self, patch: &UserQuotaPatch) {
        if let Some(ref v) = patch.storage_quota_mb {
            self.storage_quota_mb = v.clone();
        }
        if let Some(ref v) = patch.rate_read {
            self.rate_read = v.clone();
        }
        if let Some(ref v) = patch.rate_write {
            self.rate_write = v.clone();
        }
        if let Some(v) = patch.rate_read_burst {
            self.rate_read_burst = v;
        }
        if let Some(v) = patch.rate_write_burst {
            self.rate_write_burst = v;
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

/// Serde helper for patch `Option<Option<u32>>`:
/// - field absent → `None` (keep existing)
/// - field `null` → `Some(None)` (reset to default)
/// - field `N` → `Some(Some(N))` (set value)
fn deserialize_patch_option_u32<'de, D>(d: D) -> Result<Option<Option<u32>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u32>::deserialize(d).map(Some)
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
    /// Per-user read rate limit.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub rate_read: Option<QuotaOverride<BandwidthRate>>,
    /// Per-user write rate limit.
    #[serde(default, deserialize_with = "deserialize_patch_override")]
    pub rate_write: Option<QuotaOverride<BandwidthRate>>,
    /// Burst for read speed limit (in the rate's natural unit). null = reset to default.
    #[serde(default, deserialize_with = "deserialize_patch_option_u32")]
    pub rate_read_burst: Option<Option<u32>>,
    /// Burst for write speed limit (in the rate's natural unit). null = reset to default.
    #[serde(default, deserialize_with = "deserialize_patch_option_u32")]
    pub rate_write_burst: Option<Option<u32>>,
}

impl UserQuotaPatch {
    /// Check that the patch fields are individually valid:
    /// - Rate values can be persisted (fit in the DB column).
    /// - Burst values are > 0.
    ///
    /// Note: does not check cross-field constraints (e.g. burst requires a
    /// corresponding rate). Use [`UserQuota::validate`] on the merged config.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref field) = self.rate_read {
            validate_rate_value("rate_read", field)?;
        }
        if let Some(ref field) = self.rate_write {
            validate_rate_value("rate_write", field)?;
        }
        validate_burst_value("rate_read_burst", self.rate_read_burst.flatten())?;
        validate_burst_value("rate_write_burst", self.rate_write_burst.flatten())?;
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
        let q = UserQuota::from_nullable_columns(None, None, None, None, None);
        assert_eq!(q, UserQuota::default());
    }

    #[test]
    fn test_from_nullable_columns_with_values() {
        let q = UserQuota::from_nullable_columns(
            Some(500),
            Some("100mb/m".to_string()),
            None,
            None,
            None,
        );
        assert_eq!(q.storage_quota_mb, QuotaOverride::Value(500));
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
            Some("unlimited".to_string()),
            Some("unlimited".to_string()),
            None,
            None,
        );
        assert_eq!(q.storage_quota_mb, QuotaOverride::Unlimited);
        assert_eq!(q.rate_read, QuotaOverride::Unlimited);
        assert_eq!(q.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_from_nullable_columns_mixed() {
        let q =
            UserQuota::from_nullable_columns(None, Some("10mb/s".to_string()), None, None, None);
        assert_eq!(q.storage_quota_mb, QuotaOverride::Default);
        assert_eq!(
            q.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap())
        );
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_from_nullable_columns_invalid_rate_string() {
        let q = UserQuota::from_nullable_columns(
            None,
            Some("rubbish".to_string()),
            Some("100mb/m".to_string()),
            None,
            None,
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
            Some("100r/m".to_string()),
            Some("50r/s".to_string()),
            None,
            None,
        );
        assert_eq!(q.rate_read, QuotaOverride::Default);
        assert_eq!(q.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_serde_roundtrip() {
        let q = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
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
        let json = r#"{"storage_quota_mb": null, "rate_read": null, "rate_write": null}"#;
        let q: UserQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q, UserQuota::default());
    }

    #[test]
    fn test_serde_unlimited_string() {
        let json = r#"{"storage_quota_mb": "unlimited", "rate_read": "unlimited", "rate_write": "unlimited"}"#;
        let q: UserQuota = serde_json::from_str(json).unwrap();
        assert_eq!(q.storage_quota_mb, QuotaOverride::Unlimited);
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
    fn test_validate_valid_rates() {
        let budgets = ["100mb/m", "1gb/d", "500kb/s", "10mb/h", "999gb/d", "1kb/s"];
        for s in budgets {
            let q = UserQuota {
                rate_read: QuotaOverride::Value(BandwidthRate::from_str(s).unwrap()),
                rate_write: QuotaOverride::Value(BandwidthRate::from_str(s).unwrap()),
                ..Default::default()
            };
            q.validate().unwrap_or_else(|e| {
                panic!("Budget \"{s}\" should pass validation but got: {e}");
            });
        }
    }

    #[test]
    fn test_validate_skips_non_value() {
        let q = UserQuota {
            rate_read: QuotaOverride::Default,
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };
        assert!(q.validate().is_ok());
    }

    // ── Patch tests ──

    #[test]
    fn test_patch_empty_body_changes_nothing() {
        let patch: UserQuotaPatch = serde_json::from_str("{}").unwrap();
        assert!(patch.storage_quota_mb.is_none());
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
        assert!(patch.rate_read.is_none());
    }

    #[test]
    fn test_merge_applies_only_present_fields() {
        let mut base = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Value(BandwidthRate::from_str("50mb/s").unwrap()),
            ..Default::default()
        };

        // Patch only storage_quota_mb and rate_write; others should be unchanged
        let patch: UserQuotaPatch =
            serde_json::from_str(r#"{"storage_quota_mb": 200, "rate_write": "unlimited"}"#)
                .unwrap();
        base.merge(&patch);

        assert_eq!(base.storage_quota_mb, QuotaOverride::Value(200));
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
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };
        let mut patched = original.clone();
        let patch: UserQuotaPatch = serde_json::from_str("{}").unwrap();
        patched.merge(&patch);
        assert_eq!(patched, original);
    }

    #[test]
    fn test_burst_valid_with_rate() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(50),
            ..Default::default()
        };
        assert!(q.validate().is_ok());
    }

    #[test]
    fn test_burst_zero_rejected() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(0),
            ..Default::default()
        };
        let err = q.validate().unwrap_err();
        assert!(err.contains("rate_read_burst"), "error: {err}");
        assert!(err.contains("greater than 0"), "error: {err}");
    }

    #[test]
    fn test_burst_exceeds_i32_max_rejected() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(i32::MAX as u32 + 1),
            ..Default::default()
        };
        let err = q.validate().unwrap_err();
        assert!(err.contains("rate_read_burst"), "error: {err}");
        assert!(err.contains("exceeds maximum"), "error: {err}");
    }

    #[test]
    fn test_burst_at_i32_max_accepted() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(i32::MAX as u32),
            ..Default::default()
        };
        assert!(q.validate().is_ok());
    }

    #[test]
    fn test_burst_without_rate_rejected() {
        let q = UserQuota {
            rate_read: QuotaOverride::Default,
            rate_read_burst: Some(50),
            ..Default::default()
        };
        let err = q.validate().unwrap_err();
        assert!(err.contains("rate_read_burst"), "error: {err}");
    }

    #[test]
    fn test_burst_with_unlimited_rate_rejected() {
        let q = UserQuota {
            rate_write: QuotaOverride::Unlimited,
            rate_write_burst: Some(50),
            ..Default::default()
        };
        let err = q.validate().unwrap_err();
        assert!(err.contains("rate_write_burst"), "error: {err}");
    }

    #[test]
    fn test_burst_none_always_valid() {
        // No burst set — valid regardless of rate state
        for rate in [
            QuotaOverride::Default,
            QuotaOverride::Unlimited,
            QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
        ] {
            let q = UserQuota {
                rate_read: rate,
                rate_read_burst: None,
                ..Default::default()
            };
            assert!(q.validate().is_ok());
        }
    }

    #[test]
    fn test_patch_burst_zero_rejected() {
        let patch: UserQuotaPatch = serde_json::from_str(r#"{"rate_read_burst": 0}"#).unwrap();
        let err = patch.validate().unwrap_err();
        assert!(err.contains("rate_read_burst"), "error: {err}");
        assert!(err.contains("greater than 0"), "error: {err}");
    }

    #[test]
    fn test_patch_burst_null_valid() {
        // null → Some(None) → reset to default, always valid
        let patch: UserQuotaPatch = serde_json::from_str(r#"{"rate_read_burst": null}"#).unwrap();
        assert!(patch.validate().is_ok());
    }

    #[test]
    fn test_patch_burst_positive_valid() {
        let patch: UserQuotaPatch = serde_json::from_str(r#"{"rate_read_burst": 50}"#).unwrap();
        assert!(patch.validate().is_ok());
    }

    #[test]
    fn test_burst_serde_roundtrip() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(50),
            rate_write: QuotaOverride::Value(BandwidthRate::from_str("5mb/s").unwrap()),
            rate_write_burst: Some(25),
            ..Default::default()
        };
        let json = serde_json::to_string(&q).unwrap();
        let deserialized: UserQuota = serde_json::from_str(&json).unwrap();
        assert_eq!(q, deserialized);
        assert_eq!(deserialized.rate_read_burst, Some(50));
        assert_eq!(deserialized.rate_write_burst, Some(25));
    }

    #[test]
    fn test_burst_db_roundtrip() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(50),
            rate_write: QuotaOverride::Default,
            rate_write_burst: None,
            ..Default::default()
        };
        // Simulate DB write/read cycle
        let reconstructed = UserQuota::from_nullable_columns(
            q.storage_quota_mb_i64(),
            q.rate_read_str(),
            q.rate_write_str(),
            q.rate_read_burst_i32(),
            q.rate_write_burst_i32(),
        );
        assert_eq!(q, reconstructed);
    }

    #[test]
    fn test_burst_absent_from_json_when_none() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(!json.contains("rate_read_burst"));
    }

    #[test]
    fn test_burst_present_in_json_when_set() {
        let q = UserQuota {
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("10mb/s").unwrap()),
            rate_read_burst: Some(50),
            ..Default::default()
        };
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains(r#""rate_read_burst":50"#));
    }
}
