use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tracing_subscriber::filter::{Directive, LevelFilter};

/// Validated domain name according to RFC 1123.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub struct LogLevel(pub Directive);


impl FromStr for LogLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.contains("=") {
            // LevelFilter has stricter rules, so it checks them by parsing it
            let _level: LevelFilter = s
                .parse()
                .map_err(|_| anyhow::anyhow!("invalid log level directive: {}", s))?;
        };
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

impl From<LogLevel> for Directive {
    fn from(val: LogLevel) -> Self {
        val.0
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

    #[test]
    fn test_log_directive_validation() {
        // Test valid domains
        let valid_log_levels = ["dummy::dummy=TRACE", "foo_bar::foo=Debug", "bar=info"];

        for level in valid_log_levels {
            let result: anyhow::Result<LogLevel> = level.parse();
            assert!(
                result.is_ok(),
                "LogLevel spec with module '{}' should be valid",
                level
            );
        }

        let invalid_directive_levels = [("dummy::dummy=anything", "irrelevant log filter")];

        for (level, reason) in invalid_directive_levels {
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
