use std::path::{Path, PathBuf};

/// The file system config. Files are stored on the local file system.
/// The root_directory is the path the files are stored in.
/// Relative paths are expanded with the data directory path.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileSystemConfig {
    /// The root directory to use.
    #[serde(default = "default_root_directory")]
    pub root_directory: String,
}

fn default_root_directory() -> String {
    "./data/files/".to_string()
}

impl Default for FileSystemConfig {
    fn default() -> Self {
        Self {
            root_directory: default_root_directory(),
        }
    }
}

impl FileSystemConfig {
    /// Expands the `DATA_DIRECTORY_PLACEHOLDER` variable with the given data directory.
    pub fn expand_with_data_directory(&mut self, data_directory: &Path) {
        let path = PathBuf::from(&self.root_directory);

        if path.is_relative() {
            let joined_path = data_directory.join(path);
            // Normalize the path to remove any '.' components
            let normalized_path: PathBuf = joined_path.components().collect();
            self.root_directory = normalized_path.to_str().unwrap_or_default().to_string();
        }
    }

    /// Returns the builder for the file system. This will create the directory if it doesn't exist.
    /// Make sure to call `expand_with_data_directory` before using this method.
    pub fn to_builder(&self) -> Result<opendal::services::Fs, std::io::Error> {
        let path = PathBuf::from(&self.root_directory);
        if !path.exists() {
            std::fs::create_dir_all(&path)?;
        }

        let builder = opendal::services::Fs::default().root(&self.root_directory);
        Ok(builder)
    }

    /// Returns a filesystem config in a temporary directory
    /// tempdir must be kept alive for the duration of the test.
    /// As soon as tempdir is dropped, the directory is deleted.
    /// This is useful for testing.
    #[cfg(test)]
    pub(crate) fn test() -> (Self, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = Self {
            root_directory: temp_dir.path().as_os_str().to_string_lossy().to_string(),
        };

        (config, temp_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_with_data_directory_relative_path() {
        let mut config = FileSystemConfig::default();
        config.root_directory = "./data/files".to_string();
        config.expand_with_data_directory(Path::new("/root/.pubky"));
        assert_eq!(config.root_directory, "/root/.pubky/data/files");
    }

    #[test]
    fn test_expand_with_data_directory_absolute_path() {
        let mut config = FileSystemConfig::default();
        config.root_directory = "/root/my_files".to_string();
        config.expand_with_data_directory(Path::new("/root/.pubky"));
        assert_eq!(config.root_directory, "/root/my_files");
    }
}
