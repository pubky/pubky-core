use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use super::rate_unit::RateUnit;
use super::QuotaValue;

/// Per-user bandwidth budget — total bytes allowed per time window.
///
/// Unlike [`QuotaValue`] (which supports both request-count rates like `"100r/m"`
/// and bandwidth rates like `"5mb/s"`), `BandwidthBudget` only accepts bandwidth
/// units (`kb`, `mb`, `gb`). Request-count units are rejected at parse time.
///
/// This distinction exists because the two types serve different purposes:
/// - `QuotaValue` is used by the global IP rate limiter, which does both request
///   counting and bandwidth throttling (stream-level speed caps).
/// - `BandwidthBudget` is used for per-user resource accounting, where the
///   meaningful measure is total bytes transferred, not request count.
///
/// Examples: `"500mb/d"`, `"2gb/h"`, `"10mb/m"`
/// Invalid:  `"100r/m"` (request units not accepted)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BandwidthBudget(QuotaValue);

impl BandwidthBudget {
    /// Total bytes allowed per window.
    ///
    /// Calculated as `rate * unit_multiplier_kb * 1024`.
    /// The multiplier from `SpeedRateUnit` is in kilobytes, so we multiply by 1024
    /// to get bytes.
    pub fn budget_bytes(&self) -> u64 {
        let rate = self.0.rate.get() as u64;
        let unit_kb = self.0.rate_unit.multiplier().get() as u64;
        rate.saturating_mul(unit_kb).saturating_mul(1024)
    }

    /// Duration of one budget window.
    pub fn window_duration(&self) -> Duration {
        Duration::from_secs(self.0.time_unit.multiplier_in_seconds().get() as u64)
    }
}

impl fmt::Display for BandwidthBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for BandwidthBudget {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let qv: QuotaValue = s.parse()?;
        if qv.rate_unit == RateUnit::Request {
            return Err(format!(
                "BandwidthBudget does not accept request units: \"{s}\". \
                 Use a bandwidth unit (kb, mb, gb) instead, e.g. \"500mb/d\"."
            ));
        }
        Ok(BandwidthBudget(qv))
    }
}

impl serde::Serialize for BandwidthBudget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for BandwidthBudget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        BandwidthBudget::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_bandwidth_budgets() {
        let b = BandwidthBudget::from_str("500mb/d").unwrap();
        assert_eq!(b.budget_bytes(), 500 * 1024 * 1024);
        assert_eq!(b.window_duration(), Duration::from_secs(86400));

        let b = BandwidthBudget::from_str("2gb/h").unwrap();
        assert_eq!(b.budget_bytes(), 2 * 1024 * 1024 * 1024);
        assert_eq!(b.window_duration(), Duration::from_secs(3600));

        let b = BandwidthBudget::from_str("10mb/m").unwrap();
        assert_eq!(b.budget_bytes(), 10 * 1024 * 1024);
        assert_eq!(b.window_duration(), Duration::from_secs(60));

        let b = BandwidthBudget::from_str("100kb/s").unwrap();
        assert_eq!(b.budget_bytes(), 100 * 1024);
        assert_eq!(b.window_duration(), Duration::from_secs(1));
    }

    #[test]
    fn test_request_units_rejected() {
        let err = BandwidthBudget::from_str("100r/m").unwrap_err();
        assert!(err.contains("request units"), "error: {err}");
    }

    #[test]
    fn test_display_roundtrip() {
        let b = BandwidthBudget::from_str("500mb/d").unwrap();
        assert_eq!(b.to_string(), "500mb/d");

        let b2: BandwidthBudget = b.to_string().parse().unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_serde_roundtrip() {
        let b = BandwidthBudget::from_str("500mb/d").unwrap();
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"500mb/d\"");
        let b2: BandwidthBudget = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_serde_rejects_request_units() {
        let result: Result<BandwidthBudget, _> = serde_json::from_str("\"100r/m\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_budget_bytes_saturating() {
        let b = BandwidthBudget::from_str("999999gb/d").unwrap();
        assert_eq!(b.budget_bytes(), 999_999 * 1_048_576 * 1024);
    }
}
