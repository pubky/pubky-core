use std::{num::NonZeroU32, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SpeedRateUnit {
    /// Kilobyte
    Kilobyte,
    /// Megabyte
    Megabyte,
    /// Gigabyte
    Gigabyte,
}

impl SpeedRateUnit {
    /// Returns the number of bytes for this unit
    pub const fn multiplier(&self) -> NonZeroU32 {
        match self {
            // Speed quotas are always in kilobytes.
            // Because counting bytes is not practical and we are limited to u32 = 4GB max.
            // Counting in kb as more practical and we can count up to 4GB*1024 = 4TB.
            SpeedRateUnit::Kilobyte => NonZeroU32::new(1).expect("Is always non-zero"),
            SpeedRateUnit::Megabyte => NonZeroU32::new(1024).expect("Is always non-zero"),
            SpeedRateUnit::Gigabyte => NonZeroU32::new(1024 * 1024).expect("Is always non-zero"),
        }
    }
}

impl std::fmt::Display for SpeedRateUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SpeedRateUnit::Kilobyte => "kb",
                SpeedRateUnit::Megabyte => "mb",
                SpeedRateUnit::Gigabyte => "gb",
            }
        )
    }
}

impl FromStr for SpeedRateUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "kb" => Ok(SpeedRateUnit::Kilobyte),
            "mb" => Ok(SpeedRateUnit::Megabyte),
            "gb" => Ok(SpeedRateUnit::Gigabyte),
            _ => Err(format!("Invalid speedrate unit: {}", s)),
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateUnit {
    /// Request
    Request,
    /// Speed rate unit
    SpeedRateUnit(SpeedRateUnit),
}

impl std::fmt::Display for RateUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                RateUnit::Request => "r".to_string(),
                RateUnit::SpeedRateUnit(unit) => unit.to_string(),
            }
        )
    }
}

impl FromStr for RateUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "r" => Ok(RateUnit::Request),
            other => match SpeedRateUnit::from_str(other) {
                Ok(unit) => Ok(RateUnit::SpeedRateUnit(unit)),
                Err(_) => Err(format!("Invalid rate unit: {}", s)),
            },
        }
    }
}

impl RateUnit {
    /// Returns the number of bytes for this unit
    pub const fn multiplier(&self) -> NonZeroU32 {
        match self {
            RateUnit::Request => NonZeroU32::new(1).expect("Is always non-zero"),
            RateUnit::SpeedRateUnit(unit) => unit.multiplier(),
        }
    }
}
