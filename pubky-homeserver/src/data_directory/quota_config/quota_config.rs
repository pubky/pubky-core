use std::fmt;
use std::str::FromStr;

use super::quota_value::QuotaValue;
use super::rate_unit::RateUnit;
use super::{LimitKey, TimeUnit};

/// A struct that (de)serializes a Quota and LimitKey from a string.
///
/// Examples for user limit:
/// - 5 request per minute: "user:5r/m"
/// - 5 request per minute with 1 burst: "user:5r/m-1burst"
/// - 5 request per second: "user:5r/s"
/// - 5 request per second with 1 burst: "user:5r/s-1burst"
///
/// Examples for ip limit:
/// - 5 request per minute: "ip:5r/m"
/// - 5 request per minute with 1 burst: "ip:5r/m-1burst"
/// - 5 request per second: "ip:5r/s"
/// - 5 request per second with 1 burst: "ip:5r/s-1burst"
/// 
/// Examples for rate limiting by transfer speed:
/// - 5 kilobyte per minute: "user:5kb/m"
/// - 5 megabyte per minute with 1 burst: "user:5mb/m-1burst"
/// - 5 gigabyte per second: "user:5gb/s"
/// 
#[derive(Debug, Clone)]
pub struct QuotaConfig {
    /// The key to limit the quota on.
    pub limit_key: LimitKey,
    /// The value of the quota.
    pub quota_value: QuotaValue,
}

impl QuotaConfig {
    /// Create a new QuotaConfig
    pub fn new(quota_value: QuotaValue, limit_key: LimitKey) -> Self {
        Self { quota_value, limit_key }
    }

}

impl PartialEq for QuotaConfig {
    fn eq(&self, other: &Self) -> bool {
        self.quota_value == other.quota_value && self.limit_key == other.limit_key
    }
}

impl Eq for QuotaConfig {}

impl fmt::Display for QuotaConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}",
            self.limit_key,
            self.quota_value
        )
    }
}

impl FromStr for QuotaConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Split by the first colon to get limit_key and quota_value parts
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid quota config format: '{}', expected {{limit_key}}:{{quota_value}}", s));
        }
        
        let limit_key_str = parts[0];
        let quota_value_str = parts[1];
        
        // Parse the limit key
        let limit_key = LimitKey::from_str(limit_key_str)?;
        
        // Parse the quota value
        let quota_value = QuotaValue::from_str(quota_value_str)?;
        
        Ok(Self {
            limit_key,
            quota_value,
        })
    }
}

impl serde::Serialize for QuotaConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for QuotaConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Parse the quota string
        QuotaConfig::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use crate::quota_config::rate_unit::SpeedRateUnit;

    use super::*;

    #[test]
    fn test_quota_config_from_str() {
        // Test user limit
        let config = QuotaConfig::from_str("user:5r/s").unwrap();
        assert_eq!(config.limit_key, LimitKey::User);
        assert_eq!(config.quota_value.rate.get(), 5);
        assert_eq!(config.quota_value.rate_unit, RateUnit::Request);
        assert_eq!(config.quota_value.time_unit, TimeUnit::Second);
        assert_eq!(config.quota_value.burst, None);

        // Test IP limit with burst
        let config = QuotaConfig::from_str("ip:10r/m-2burst").unwrap();
        assert_eq!(config.limit_key, LimitKey::Ip);
        assert_eq!(config.quota_value.rate.get(), 10);
        assert_eq!(config.quota_value.rate_unit, RateUnit::Request);
        assert_eq!(config.quota_value.time_unit, TimeUnit::Minute);
        assert!(config.quota_value.burst.is_some());
        assert_eq!(config.quota_value.burst.unwrap().0.get(), 2);

        // Test with data rate
        let config = QuotaConfig::from_str("user:5mb/s").unwrap();
        assert_eq!(config.limit_key, LimitKey::User);
        assert_eq!(config.quota_value.rate.get(), 5);
        assert_eq!(config.quota_value.rate_unit, RateUnit::SpeedRateUnit(SpeedRateUnit::Megabyte));
        assert_eq!(config.quota_value.time_unit, TimeUnit::Second);
        assert_eq!(config.quota_value.burst, None);
    }

    #[test]
    fn test_quota_config_invalid_formats() {
        // Missing colon
        assert!(QuotaConfig::from_str("user5r/s").is_err());
        
        // Invalid limit key
        assert!(QuotaConfig::from_str("invalid:5r/s").is_err());
        
        // Invalid quota value
        assert!(QuotaConfig::from_str("user:5x/s").is_err());
    }

    #[test]
    fn test_quota_config_display() {
        let config = QuotaConfig::from_str("user:5r/s").unwrap();
        assert_eq!(config.to_string(), "user:5r/s");
        
        let config = QuotaConfig::from_str("ip:10r/m-2burst").unwrap();
        assert_eq!(config.to_string(), "ip:10r/m-2burst");
    }
}
