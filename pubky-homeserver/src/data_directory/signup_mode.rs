use core::fmt;
use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// The mode of signup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SignupMode {
    /// Everybody can signup.
    Open,
    /// Only users with a valid token can signup.
    #[default]
    TokenRequired,
}

impl Display for SignupMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::TokenRequired => write!(f, "token_required"),
        }
    }
}

impl FromStr for SignupMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "open" => Self::Open,
            "token_required" => Self::TokenRequired,
            _ => return Err(anyhow::anyhow!("Invalid signup mode: {}", s)),
        })
    }
}

impl Serialize for SignupMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

impl<'de> Deserialize<'de> for SignupMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str(&s).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signup_mode_from_str() {
        assert_eq!(SignupMode::from_str("open").unwrap(), SignupMode::Open);
        assert_eq!(
            SignupMode::from_str("token_required").unwrap(),
            SignupMode::TokenRequired
        );
    }

    #[test]
    fn test_signup_mode_display() {
        assert_eq!(SignupMode::Open.to_string(), "open");
        assert_eq!(SignupMode::TokenRequired.to_string(), "token_required");
    }
}
