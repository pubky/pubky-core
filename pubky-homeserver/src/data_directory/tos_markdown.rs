use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};

/// A validated and loaded Terms of Service markdown file.
#[derive(Debug, Clone, PartialEq)]
pub struct TosMarkdown {
    #[allow(dead_code)]
    path: PathBuf,
    cached_content: String,
}

impl TosMarkdown {
    /// Gets the cached content of the ToS file.
    pub fn content(&self) -> &str {
        &self.cached_content
    }
}

/// Custom deserializer to handle an optional path string.
/// An empty string in the config will result in `Ok(None)`.
pub fn deserialize_optional_tos<'de, D>(deserializer: D) -> Result<Option<TosMarkdown>, D::Error>
where
    D: Deserializer<'de>,
{
    let path_str = String::deserialize(deserializer)?;
    if path_str.is_empty() {
        return Ok(None);
    }

    let path = Path::new(&path_str);

    // Validate the path
    if !path.exists() {
        return Err(serde::de::Error::custom(format!(
            "ToS file not found at '{}'",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(serde::de::Error::custom(format!(
            "'{}' is not a file",
            path.display()
        )));
    }
    if path.extension().and_then(|s| s.to_str()) != Some("md") {
        return Err(serde::de::Error::custom(
            "Terms of Service file must have a .md extension",
        ));
    }

    // Read and cache the content
    let content = std::fs::read_to_string(path).map_err(serde::de::Error::custom)?;

    Ok(Some(TosMarkdown {
        path: path.to_path_buf(),
        cached_content: content,
    }))
}
