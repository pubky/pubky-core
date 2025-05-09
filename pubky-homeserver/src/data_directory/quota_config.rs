use governor::Quota;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// The key to limit the quota on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitKey {
    /// Limit on the user id    
    User,
    /// Limit on the ip address
    Ip,
}

impl fmt::Display for LimitKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            LimitKey::User => "user",
            LimitKey::Ip => "ip",
        })
    }
}

impl FromStr for LimitKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(LimitKey::User),
            "ip" => Ok(LimitKey::Ip),
            _ => Err(format!("Invalid limit key: {}", s)),
        }
    }
}

/// The time unit of the quota.
///
/// Examples:
/// - "s" -> second
/// - "m" -> minute
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeUnit {
    /// Second
    Second,
    /// Minute
    Minute,
}

impl fmt::Display for TimeUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            TimeUnit::Second => "s",
            TimeUnit::Minute => "m",
        })
    }
}

impl FromStr for TimeUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s" => Ok(TimeUnit::Second),
            "m" => Ok(TimeUnit::Minute),
            _ => Err(format!("Invalid time unit: {}", s)),
        }
    }
}

/// The unit of the rate.
///
/// Examples:
/// - "r" -> request
/// - "kb" -> kilobyte
/// - "mb" -> megabyte
/// - "gb" -> gigabyte
/// - "tb" -> terabyte
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateUnit {
    /// Request
    Request, 
    /// Kilobyte
    Kilobyte,
    /// Megabyte
    Megabyte,
    /// Gigabyte
    Gigabyte,
    /// Terabyte
    Terabyte,
}

impl fmt::Display for RateUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            RateUnit::Request => "r",
            RateUnit::Kilobyte => "kb",
            RateUnit::Megabyte => "mb",
            RateUnit::Gigabyte => "gb",
            RateUnit::Terabyte => "tb",
        })
    }
}

impl FromStr for RateUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "r" => Ok(RateUnit::Request),
            "kb" => Ok(RateUnit::Kilobyte),
            "mb" => Ok(RateUnit::Megabyte),
            "gb" => Ok(RateUnit::Gigabyte),
            "tb" => Ok(RateUnit::Terabyte),
            _ => Err(format!("Invalid rate unit: {}", s)),
        }
    }
}

/// A struct that (de)serializes a Quota and LimitKey from a string.
///
/// Examples for user limit:
/// - 5 request per minute: "user:5r/m"
/// - 5 request per minute with 1 burst: "user:5r/m:1burst"
/// - 5 request per second: "user:5r/s"
/// - 5 request per second with 1 burst: "user:5r/s:1burst"
///
/// Examples for ip limit:
/// - 5 request per minute: "ip:5r/m"
/// - 5 request per minute with 1 burst: "ip:5r/m:1burst"
/// - 5 request per second: "ip:5r/s"
/// - 5 request per second with 1 burst: "ip:5r/s:1burst"
/// 
/// Examples for rate limiting by transfer speed:
/// - 5 kilobyte per minute: "user:5kb/m"
/// - 5 megabyte per minute with 1 burst: "user:5mb/m:1burst"
/// - 5 gigabyte per second: "user:5gb/s"
/// - 5 terabyte per second with 1 burst: "user:5tb/s:1burst"
/// 
#[derive(Debug, Clone)]
pub struct QuotaConfig {
    /// The quota to apply.
    pub quota: Quota,
    /// The key to limit the quota on.
    pub limit_key: LimitKey,
    /// The unit of the rate.
    pub rate_unit: RateUnit,
}

impl QuotaConfig {
    /// Create a new QuotaConfig
    pub fn new(quota: Quota, limit_key: LimitKey, rate_unit: RateUnit) -> Self {
        Self { quota, limit_key, rate_unit }
    }

    /// Get the time unit of the quota.
    fn quota_time_unit(&self) -> TimeUnit {
        let replenish_interval = self.quota.replenish_interval().as_nanos();
        if replenish_interval < 1_000_000_000 {
            TimeUnit::Second
        } else {
            TimeUnit::Minute
        }
    }

    /// Get the count of the quota.
    fn quota_count(&self) -> u32 {
        let replenish_interval = self.quota.replenish_interval().as_nanos();
        match self.quota_time_unit() {
            TimeUnit::Second => {
                (Duration::from_secs(1).as_nanos() / replenish_interval) as u32
            },
            TimeUnit::Minute => {
                (Duration::from_secs(60).as_nanos() / replenish_interval) as u32
            },
        }
    }
}

impl PartialEq for QuotaConfig {
    fn eq(&self, other: &Self) -> bool {
        self.quota == other.quota && self.limit_key == other.limit_key && self.rate_unit == other.rate_unit
    }
}

impl Eq for QuotaConfig {}

impl fmt::Display for QuotaConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let burst_size = self.quota.burst_size().get();
        let time_unit = self.quota_time_unit();
        let count = self.quota_count();

        if burst_size == count {
            write!(f, "{}:{}{}/{}",
                self.limit_key,
                count,
                self.rate_unit,
                time_unit
            )
        } else {
            write!(f, "{}:{}{}/{}:{}burst",
                self.limit_key,
                count,
                self.rate_unit,
                time_unit,
                burst_size
            )
        }
    }
}

impl FromStr for QuotaConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Parse formats like "ip:5r/s", "ip:5r/s:1burst", "user:5kb/m", "user:5mb/m:1burst"
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() < 2 {
            return Err(format!("Invalid quota format: {}", s));
        }

        // Parse the limit key
        let limit_key = LimitKey::from_str(parts[0])?;

        // Get the rate part (e.g. "5r/s" or "5kb/m")
        let rate_part = parts[1];

        // Split by '/' to get the count+unit and time unit
        let rate_parts: Vec<&str> = rate_part.split('/').collect();
        if rate_parts.len() != 2 {
            return Err(format!("Invalid quota format: {}", s));
        }

        // Parse count and rate unit
        let count_part = rate_parts[0];
        
        // Find where the digits end and the unit starts
        let digit_end = count_part.chars().take_while(|c| c.is_digit(10)).count();
        if digit_end == 0 {
            return Err(format!("Invalid count in quota: no digits found in {}", count_part));
        }
        
        // Parse count
        let count: u32 = count_part[..digit_end]
            .parse()
            .map_err(|_| format!("Invalid count in quota: {}", &count_part[..digit_end]))?;
        
        // Parse rate unit
        if digit_end >= count_part.len() {
            return Err(format!("Missing rate unit in quota: {}", count_part));
        }
        
        let rate_unit_str = &count_part[digit_end..];
        let rate_unit = RateUnit::from_str(rate_unit_str)?;

        // Parse time unit
        let time_unit = TimeUnit::from_str(rate_parts[1])
            .map_err(|e| format!("Invalid time unit in quota: {}", e))?;

        // Create base quota
        let quota = match time_unit {
            TimeUnit::Second => Quota::per_second(
                count
                    .try_into()
                    .map_err(|_| format!("Invalid count: {}", count))?,
            ),
            TimeUnit::Minute => Quota::per_minute(
                count
                    .try_into()
                    .map_err(|_| format!("Invalid count: {}", count))?,
            ),
        };

        // Check if burst is specified
        if parts.len() > 2 {
            // Extract burst size
            let burst_part = parts[2];
            if !burst_part.ends_with("burst") {
                return Err(format!("Invalid burst format: {}", burst_part));
            }

            let burst_str = &burst_part[0..burst_part.len() - 5]; // remove "burst"
            let burst_size: u32 = burst_str
                .parse()
                .map_err(|_| format!("Invalid burst size: {}", burst_str))?;

            Ok(QuotaConfig {
                quota: quota.allow_burst(
                    burst_size
                        .try_into()
                        .map_err(|_| format!("Invalid burst size: {}", burst_size))?,
                ),
                limit_key,
                rate_unit,
            })
        } else {
            Ok(QuotaConfig { quota, limit_key, rate_unit })
        }
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
    use super::*;

    #[tokio::test]
    async fn test_serialize_quota_minute_no_burst_ip() {
        let quota = Quota::per_minute(5.try_into().unwrap());
        let result = QuotaConfig::new(quota, LimitKey::Ip, RateUnit::Request).to_string();
        assert_eq!(result, "ip:5r/m");
    }

    #[tokio::test]
    async fn test_serialize_quota_minute_no_burst_user() {
        let quota = Quota::per_minute(5.try_into().unwrap());
        let result = QuotaConfig::new(quota, LimitKey::User, RateUnit::Request).to_string();
        assert_eq!(result, "user:5r/m");
    }

    #[tokio::test]
    async fn test_serialize_quota_minute_with_burst() {
        let quota = Quota::per_minute(5.try_into().unwrap()).allow_burst(1.try_into().unwrap());
        let result = QuotaConfig::new(quota, LimitKey::Ip, RateUnit::Request).to_string();
        assert_eq!(result, "ip:5r/m:1burst");
    }

    #[tokio::test]
    async fn test_serialize_quota_second_no_burst() {
        let quota = Quota::per_second(5.try_into().unwrap());
        let result = QuotaConfig::new(quota, LimitKey::Ip, RateUnit::Request).to_string();
        assert_eq!(result, "ip:5r/s");
    }

    #[tokio::test]
    async fn test_serialize_quota_second_with_burst() {
        let quota = Quota::per_second(5.try_into().unwrap()).allow_burst(1.try_into().unwrap());
        let result = QuotaConfig::new(quota, LimitKey::Ip, RateUnit::Request).to_string();
        assert_eq!(result, "ip:5r/s:1burst");
    }

    #[tokio::test]
    async fn test_fromstr_quota_minute_no_burst() {
        let quota = QuotaConfig::from_str("user:5r/m").unwrap();
        assert_eq!(quota.quota, Quota::per_minute(5.try_into().unwrap()));
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Request));
    }

    #[tokio::test]
    async fn test_fromstr_quota_minute_with_burst() {
        let quota = QuotaConfig::from_str("user:5r/m:1burst").unwrap();
        assert_eq!(
            quota.quota,
            Quota::per_minute(5.try_into().unwrap()).allow_burst(1.try_into().unwrap())
        );
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Request));
    }

    #[tokio::test]
    async fn test_fromstr_quota_second_no_burst() {
        let quota = QuotaConfig::from_str("user:5r/s").unwrap();
        assert_eq!(quota.quota, Quota::per_second(5.try_into().unwrap()));
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Request));
    }

    #[tokio::test]
    async fn test_fromstr_quota_second_with_burst() {
        let quota = QuotaConfig::from_str("user:5r/s:1burst").unwrap();
        assert_eq!(
            quota.quota,
            Quota::per_second(5.try_into().unwrap()).allow_burst(1.try_into().unwrap())
        );
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Request));
    }

    #[tokio::test]
    async fn test_rate_unit_kb() {
        let quota = QuotaConfig::from_str("user:5kb/s").unwrap();
        assert_eq!(quota.quota, Quota::per_second(5.try_into().unwrap()));
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Kilobyte));
        assert_eq!(quota.to_string(), "user:5kb/s");
    }

    #[tokio::test]
    async fn test_rate_unit_mb() {
        let quota = QuotaConfig::from_str("ip:5mb/m:10burst").unwrap();
        assert_eq!(
            quota.quota,
            Quota::per_minute(5.try_into().unwrap()).allow_burst(10.try_into().unwrap())
        );
        assert!(matches!(quota.limit_key, LimitKey::Ip));
        assert!(matches!(quota.rate_unit, RateUnit::Megabyte));
        assert_eq!(quota.to_string(), "ip:5mb/m:10burst");
    }

    #[tokio::test]
    async fn test_rate_unit_gb() {
        let quota = QuotaConfig::from_str("user:5gb/s").unwrap();
        assert_eq!(quota.quota, Quota::per_second(5.try_into().unwrap()));
        assert!(matches!(quota.limit_key, LimitKey::User));
        assert!(matches!(quota.rate_unit, RateUnit::Gigabyte));
        assert_eq!(quota.to_string(), "user:5gb/s");
    }

    #[tokio::test]
    async fn test_rate_unit_tb() {
        let quota = QuotaConfig::from_str("ip:5tb/m").unwrap();
        assert_eq!(quota.quota, Quota::per_minute(5.try_into().unwrap()));
        assert!(matches!(quota.limit_key, LimitKey::Ip));
        assert!(matches!(quota.rate_unit, RateUnit::Terabyte));
        assert_eq!(quota.to_string(), "ip:5tb/m");
    }

    #[tokio::test]
    async fn test_deserialize_invalid_format() {
        let invalid_formats = [
            "invalid",
            "user:5m",              // Missing time unit separator
            "ip:5m",                // Missing time unit separator
            "user:5:burst",         // Missing rate and time unit
            "ip:5:burst",           // Missing rate and time unit
            "user:5/z",             // Invalid time unit
            "ip:5/z",               // Invalid time unit
            "user:5/s:invalid",     // Invalid burst format
            "ip:5/s:invalid",       // Invalid burst format
            "unknown:5r/s",         // Invalid limit key
            "5r/s",                 // Missing limit key
            ":5r/s",                // Empty limit key
            "user:",                // Missing rate
            "user:/s",              // Missing count
            "ip:/m",                // Missing count
            "user:5xyz/s",          // Invalid rate unit
            "user:5/s",             // Missing rate unit (should be "user:5r/s")
            "ip:5/m",               // Missing rate unit (should be "ip:5r/m")
            "user:5k/s",            // Incomplete rate unit (kb)
            "ip:5m/m",              // Incomplete rate unit (mb)
            "user:5g/s",            // Incomplete rate unit (gb)
            "ip:5t/m",              // Incomplete rate unit (tb)
            "user:xkb/s",           // Non-numeric count
            "ip:5.5kb/m",           // Float instead of integer
            "user:5kbs/s",          // Invalid rate unit
            "ip:5mbz/m",            // Invalid rate unit
            "user:kb/s",            // Missing count
            "ip:mb/m:burst",        // Missing burst size
            "user:5r/m:0burst",     // Zero burst size
            "user:0r/s",            // Zero count
            "ip:5kb/s:1.5burst",    // Non-integer burst
        ];

        for format in invalid_formats {
            let result = QuotaConfig::from_str(format);
            assert!(result.is_err(), "Should fail on invalid format: {}", format);
        }
    }
}
