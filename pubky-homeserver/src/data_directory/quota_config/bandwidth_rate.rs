use std::fmt;
use std::str::FromStr;

use super::rate_unit::RateUnit;
use super::QuotaValue;

/// Per-user bandwidth rate — a speed limit for governor throttling.
///
/// Unlike [`QuotaValue`] (which supports both request-count rates like `"100r/m"`
/// and bandwidth rates like `"5mb/s"`), `BandwidthRate` only accepts bandwidth
/// units (`kb`, `mb`, `gb`). Request-count units are rejected at parse time.
///
/// Used as per-user speed limit overrides in the path-based `RateLimiterLayer`.
/// When a user has a custom `BandwidthRate`, their traffic is throttled at
/// that speed instead of the default path limit.
///
/// Examples: `"10mb/s"`, `"1gb/s"`, `"500kb/s"`
/// Invalid:  `"100r/m"` (request units not accepted)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BandwidthRate(QuotaValue);

impl From<BandwidthRate> for governor::Quota {
    fn from(value: BandwidthRate) -> Self {
        value.0.into()
    }
}

impl fmt::Display for BandwidthRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for BandwidthRate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let qv: QuotaValue = s.parse()?;
        if qv.rate_unit == RateUnit::Request {
            return Err(format!(
                "BandwidthRate does not accept request units: \"{s}\". \
                 Use a bandwidth unit (kb, mb, gb) instead, e.g. \"10mb/s\"."
            ));
        }
        Ok(BandwidthRate(qv))
    }
}

impl serde::Serialize for BandwidthRate {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for BandwidthRate {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        BandwidthRate::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_bandwidth_rates() {
        let b = BandwidthRate::from_str("10mb/s").unwrap();
        assert_eq!(b.to_string(), "10mb/s");

        let b = BandwidthRate::from_str("1gb/s").unwrap();
        assert_eq!(b.to_string(), "1gb/s");

        let b = BandwidthRate::from_str("500kb/s").unwrap();
        assert_eq!(b.to_string(), "500kb/s");

        // Rates with longer time windows still work
        let b = BandwidthRate::from_str("500mb/d").unwrap();
        assert_eq!(b.to_string(), "500mb/d");
    }

    #[test]
    fn test_request_units_rejected() {
        let err = BandwidthRate::from_str("100r/m").unwrap_err();
        assert!(err.contains("request units"), "error: {err}");
    }

    #[test]
    fn test_display_roundtrip() {
        let b = BandwidthRate::from_str("10mb/s").unwrap();
        assert_eq!(b.to_string(), "10mb/s");

        let b2: BandwidthRate = b.to_string().parse().unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_serde_roundtrip() {
        let b = BandwidthRate::from_str("10mb/s").unwrap();
        let json = serde_json::to_string(&b).unwrap();
        assert_eq!(json, "\"10mb/s\"");
        let b2: BandwidthRate = serde_json::from_str(&json).unwrap();
        assert_eq!(b, b2);
    }

    #[test]
    fn test_serde_rejects_request_units() {
        let result: Result<BandwidthRate, _> = serde_json::from_str("\"100r/m\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_converts_to_governor_quota() {
        let b = BandwidthRate::from_str("10mb/s").unwrap();
        let _quota: governor::Quota = b.into();
    }
}
