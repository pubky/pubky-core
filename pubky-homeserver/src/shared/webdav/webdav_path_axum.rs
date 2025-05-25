use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::WebDavPath;

/// A webdav path that can be used with axum.
///
/// When using `.route("/{*path}", your_handler)` in axum, the path is passed without the leading slash.
/// This struct adds the leading slash back and therefore allows direct validation of the path.
///
/// Usage in handler:
///
/// `Path(path): Path<WebDavPathAxum>`
pub struct WebDavPathAxum(pub WebDavPath);

impl std::fmt::Display for WebDavPathAxum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

impl FromStr for WebDavPathAxum {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let with_slash = format!("/{}", s);
        let inner = WebDavPath::new(&with_slash)?;
        Ok(Self(inner))
    }
}

impl Serialize for WebDavPathAxum {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for WebDavPathAxum {
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
    fn test_webdav_path_axum() {
        let path = WebDavPathAxum::from_str("pub/foo/bar").unwrap();
        assert_eq!(path.0.as_str(), "/pub/foo/bar");
    }
}
