use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tracing_core::Level;

#[derive(Debug, Clone, PartialEq)]
pub struct LogLevel(pub Level);

impl FromStr for LogLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = s
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid log level directive: {}", s))?;
        Ok(Self(parsed))
    }
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for LogLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LogLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl From<LogLevel> for Level {
    fn from(val: LogLevel) -> Self {
        val.0
    }
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel(Level::INFO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_validation() {
        let valid_log_levels = ["TRACE", "Debug", "info", "warN", "eRRoR"];

        for level in valid_log_levels {
            let result: anyhow::Result<LogLevel> = level.parse();
            assert!(result.is_ok(), "LogLevel '{}' should be valid", level);
        }

        let invalid_log_levels = [("anything", "irrelevant log filter")];

        for (level, reason) in invalid_log_levels {
            let result: anyhow::Result<LogLevel> = level.parse();
            assert!(
                result.is_err(),
                "LogLevel '{}' should be invalid: {}",
                level,
                reason
            );
        }
    }
}
