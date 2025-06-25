use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::WebDavPath;

/// A webdav path that requires the leading `/pub/` segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WebDavPathPub(pub WebDavPath);

impl WebDavPathPub {
    pub fn inner(&self) -> &WebDavPath {
        &self.0
    }
}

impl std::fmt::Display for WebDavPathPub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl FromStr for WebDavPathPub {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = WebDavPath::new(s)?;
        if !inner.as_str().starts_with("/pub/") {
            return Err(anyhow::anyhow!("Path must start with /pub/"));
        }
        Ok(Self(inner))
    }
}

impl Serialize for WebDavPathPub {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for WebDavPathPub {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webdav_pub_required() {
        WebDavPathPub::from_str("/pub/file.txt").expect("Should be valid");
        WebDavPathPub::from_str("/file.txt").expect_err("Should not be valid. /pub/ required.");
    }
}
