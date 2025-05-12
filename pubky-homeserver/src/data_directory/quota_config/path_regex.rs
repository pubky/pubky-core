use std::{fmt::Display, str::FromStr};

use regex::Regex;
use serde::{Deserialize, Serialize};

/// A wrapper around regex::Regex to implement serde traits
#[derive(Debug, Clone)]
pub struct PathRegex(pub Regex);

impl std::hash::Hash for PathRegex {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.as_str().hash(state);
    }
}

impl From<Regex> for PathRegex {
    fn from(regex: Regex) -> Self {
        PathRegex(regex)
    }
}

impl From<PathRegex> for Regex {
    fn from(path_regex: PathRegex) -> Self {
        path_regex.0
    }
}

impl FromStr for PathRegex {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Regex::new(s)
            .map(PathRegex)
            .map_err(|e| format!("Invalid regex pattern: {}", e))
    }
}

impl Display for PathRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl Serialize for PathRegex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for PathRegex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PathRegex::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl PartialEq for PathRegex {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for PathRegex {}
