use std::{num::NonZeroU32, time::Duration};
use std::fmt;
use std::str::FromStr;

use super::{Burst, RateUnit, TimeUnit};

/// Quota value
/// 
/// Examples:
/// - 5r/m-1burst
/// - 5r/s-1burst
/// - 5kb/m
/// - 5mb/m-1burst
/// - 5gb/s
/// - 5tb/s-1burst
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaValue {
    pub burst: Option<Burst>,
    pub rate: NonZeroU32,
    pub rate_unit: RateUnit,
    pub time_unit: TimeUnit,
}


impl From<QuotaValue> for governor::Quota {
    /// Get the quota to do the actual rate limiting.
    /// 
    /// Important: The speed quotas are always in kilobytes, not bytes.
    /// Counting bytes is not practical.
    /// 
    fn from(value: QuotaValue) -> Self {
        let rate_count = value.rate.get();
        let rate_unit = value.rate_unit.multiplier().get();
        let rate = NonZeroU32::new(rate_count * rate_unit).expect("Is always non-zero because rate count and rate unit multiplier are non-zero");
        let time_unit = Duration::from_secs(value.time_unit.multiplier_in_seconds().get() as u64);
        let replenish_1_per = time_unit / rate.get();

        let base_quota = governor::Quota::with_period(replenish_1_per)
        .expect("Is always non-zero because replenish_1_per is non-zero");
        if let Some(burst) = &value.burst {
            let burst_size = NonZeroU32::new(burst.0.get() * value.rate_unit.multiplier().get())
            .expect("Is always non-zero because burst is non-zero and rate unit multiplier is non-zero");
            base_quota.allow_burst(burst_size)
        } else {
            base_quota.allow_burst(rate)
        }
    }
}

impl fmt::Display for QuotaValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rate_str = format!("{}{}/{}", self.rate, self.rate_unit, self.time_unit);
        if let Some(burst) = &self.burst {
            write!(f, "{}-{}", rate_str, burst)
        } else {
            write!(f, "{}", rate_str)
        }
    }
}

impl FromStr for QuotaValue {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Split by '-' to get rate part and burst part (if any)
        let parts: Vec<&str> = s.split('-').collect();
        
        let rate_part = parts[0];
        let burst = if parts.len() > 1 {
            // Parse burst if present
            Some(Burst::from_str(parts[1])?)
        } else {
            None
        };

        // Split rate part by '/' to get rate+unit and time unit
        let rate_parts: Vec<&str> = rate_part.split('/').collect();
        if rate_parts.len() != 2 {
            return Err(format!("Invalid rate format: '{}', expected {{rate}}{{unit}}/{{time}}", rate_part));
        }

        let rate_with_unit = rate_parts[0];
        let time_unit = TimeUnit::from_str(rate_parts[1])?;

        // Find the boundary between rate digits and unit
        let rate_digit_end = rate_with_unit
            .chars()
            .position(|c| !c.is_digit(10))
            .unwrap_or(rate_with_unit.len());

        if rate_digit_end == 0 {
            return Err(format!("Missing rate value in '{}'", rate_with_unit));
        }

        let rate_str = &rate_with_unit[..rate_digit_end];
        let rate_unit_str = &rate_with_unit[rate_digit_end..];

        let rate = rate_str.parse::<NonZeroU32>()
            .map_err(|_| format!("Failed to parse rate from '{}'", rate_str))?;
        let rate_unit = RateUnit::from_str(rate_unit_str)?;

        Ok(QuotaValue {
            burst,
            rate,
            rate_unit,
            time_unit,
        })
    }
}

impl serde::Serialize for QuotaValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for QuotaValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Parse the quota string
        QuotaValue::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::quota_config::rate_unit::SpeedRateUnit;

    use super::*;

    #[test]
    fn test_get_quota() {
        let quota = QuotaValue::from_str("5r/s").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_second(NonZeroU32::new(5).unwrap()));

        let quota = QuotaValue::from_str("5r/s-1burst").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_second(NonZeroU32::new(5).unwrap()).allow_burst(NonZeroU32::new(1).unwrap()));

        let quota = QuotaValue::from_str("5r/m").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_minute(NonZeroU32::new(5).unwrap()));

        let quota = QuotaValue::from_str("5r/m-1burst").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_minute(NonZeroU32::new(5).unwrap()).allow_burst(NonZeroU32::new(1).unwrap()));

        let quota = QuotaValue::from_str("5r/m").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_minute(NonZeroU32::new(5).unwrap()));

        let quota = QuotaValue::from_str("5kb/s").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_second(NonZeroU32::new(5).unwrap()));

        let quota = QuotaValue::from_str("5kb/s-1burst").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_second(NonZeroU32::new(5).unwrap()).allow_burst(NonZeroU32::new(1).unwrap()));

        let quota = QuotaValue::from_str("5mb/m").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_minute(NonZeroU32::new(5*1024).unwrap()));

        let quota = QuotaValue::from_str("5mb/m-1burst").unwrap();
        assert_eq!(governor::Quota::from(quota), governor::Quota::per_minute(NonZeroU32::new(5*1024).unwrap()).allow_burst(NonZeroU32::new(1*1024).unwrap()));
    }

    #[test]
    fn test_quota_value_from_str() {
        // Test without burst
        let quota = QuotaValue::from_str("5r/s").unwrap();
        assert_eq!(quota.rate, NonZeroU32::new(5).unwrap());
        assert_eq!(quota.rate_unit, RateUnit::Request);
        assert_eq!(quota.time_unit, TimeUnit::Second);
        assert_eq!(quota.burst, None);

        // Test with burst
        let quota = QuotaValue::from_str("10mb/m-2burst").unwrap();
        assert_eq!(quota.rate, NonZeroU32::new(10).unwrap());
        assert_eq!(quota.rate_unit, RateUnit::SpeedRateUnit(SpeedRateUnit::Megabyte));
        assert_eq!(quota.time_unit, TimeUnit::Minute);
        assert_eq!(quota.burst, Some(Burst(NonZeroU32::new(2).unwrap())));
    }

    #[test]
    fn test_quota_value_display() {
        // Test without burst
        let quota = QuotaValue {
            rate: NonZeroU32::new(5).unwrap(),
            rate_unit: RateUnit::Request,
            time_unit: TimeUnit::Second,
            burst: None,
        };
        assert_eq!(quota.to_string(), "5r/s");

        // Test with burst
        let quota = QuotaValue {
            rate: NonZeroU32::new(10).unwrap(),
            rate_unit: RateUnit::SpeedRateUnit(SpeedRateUnit::Megabyte),
            time_unit: TimeUnit::Minute,
            burst: Some(Burst(NonZeroU32::new(2).unwrap())),
        };
        assert_eq!(quota.to_string(), "10mb/m-2burst");
    }

    #[test]
    fn test_quota_value_invalid_formats() {
        // Invalid format: missing /
        assert!(QuotaValue::from_str("5rs").is_err());

        // Invalid format: missing rate
        assert!(QuotaValue::from_str("r/s").is_err());

        // Invalid format: invalid rate unit
        assert!(QuotaValue::from_str("5x/s").is_err());

        // Invalid format: invalid time unit
        assert!(QuotaValue::from_str("5r/x").is_err());

        // Invalid format: invalid burst
        assert!(QuotaValue::from_str("5r/s-2").is_err());
    }
}

