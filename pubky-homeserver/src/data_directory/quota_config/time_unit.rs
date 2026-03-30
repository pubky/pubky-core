use std::num::NonZeroU32;

/// The time unit of the quota.
///
/// Examples:
/// - "s" -> second
/// - "m" -> minute
/// - "h" -> hour
/// - "d" -> day
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    /// Second
    Second,
    /// Minute
    Minute,
    /// Hour
    Hour,
    /// Day
    Day,
}

impl std::fmt::Display for TimeUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                TimeUnit::Second => "s",
                TimeUnit::Minute => "m",
                TimeUnit::Hour => "h",
                TimeUnit::Day => "d",
            }
        )
    }
}

impl std::str::FromStr for TimeUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "s" => Ok(TimeUnit::Second),
            "m" => Ok(TimeUnit::Minute),
            "h" => Ok(TimeUnit::Hour),
            "d" => Ok(TimeUnit::Day),
            _ => Err(format!("Invalid time unit: {}", s)),
        }
    }
}

impl TimeUnit {
    /// Returns the number of seconds for each unit
    pub const fn multiplier_in_seconds(&self) -> NonZeroU32 {
        match self {
            TimeUnit::Second => NonZeroU32::new(1).expect("Is always non-zero"),
            TimeUnit::Minute => NonZeroU32::new(60).expect("Is always non-zero"),
            TimeUnit::Hour => NonZeroU32::new(3600).expect("Is always non-zero"),
            TimeUnit::Day => NonZeroU32::new(86400).expect("Is always non-zero"),
        }
    }
}
