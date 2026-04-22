use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;
use std::time::Duration;

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

impl BandwidthRate {
    /// Convert to a `governor::Quota`, optionally overriding the burst.
    ///
    /// - `burst_override = None` → default behaviour (burst = rate).
    /// - `burst_override = Some(n)` → burst is `n` in the rate's natural unit
    ///   (MB for `"…mb/s"`, KB for `"…kb/s"`, etc.).
    pub fn to_governor_quota(&self, burst_override: Option<u32>) -> governor::Quota {
        let qv = &self.0;
        let rate_unit_mult = qv.rate_unit.multiplier().get();
        let rate_cells = NonZeroU32::new(qv.rate.get() * rate_unit_mult)
            .expect("always non-zero: rate and multiplier are non-zero");
        let time_unit = Duration::from_secs(qv.time_unit.multiplier_in_seconds().get() as u64);
        let replenish_1_per = time_unit / rate_cells.get();
        let base = governor::Quota::with_period(replenish_1_per)
            .expect("always non-zero: replenish_1_per is non-zero");

        match burst_override {
            Some(b) => {
                let burst_cells =
                    NonZeroU32::new(b.saturating_mul(rate_unit_mult)).unwrap_or(rate_cells);
                base.allow_burst(burst_cells)
            }
            None => base.allow_burst(rate_cells),
        }
    }
}

impl From<BandwidthRate> for governor::Quota {
    fn from(value: BandwidthRate) -> Self {
        value.to_governor_quota(None)
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
