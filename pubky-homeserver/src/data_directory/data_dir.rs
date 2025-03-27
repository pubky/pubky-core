use super::{data_dir_trait::DataDirTrait, ConfigToml};
use std::{io::Write, os::unix::fs::PermissionsExt, path::{Path, PathBuf}, sync::Arc};

/// The data directory for the homeserver.
///
/// This is the directory that will store the homeservers data.
///
#[derive(Debug, Clone)]
pub struct DataDir {
    expanded_path: PathBuf,
    #[cfg(any(test, feature = "testing"))]
    // Only used in tests to keep the temporary directory alive
    temp_dir: Arc<Option<tempfile::TempDir>>,
}

impl DataDir {
    /// Creates a new data directory.
    /// `path` will be expanded to the home directory if it starts with "~".
    pub fn new(path: PathBuf) -> Self {
        Self {
            expanded_path: Self::expand_home_dir(path),
            #[cfg(any(test, feature = "testing"))]
            temp_dir: Arc::new(None),
        }
    }

    /// Expands the data directory to the home directory if it starts with "~".
    /// Return the full path to the data directory.
    fn expand_home_dir(path: PathBuf) -> PathBuf {
        let path = match path.to_str() {
            Some(path) => path,
            None => {
                // Path not valid utf-8 so we can't expand it.
                return path;
            }
        };

        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                let without_home = path.strip_prefix("~/").expect("Invalid ~ prefix");
                let joined = home.join(without_home);
                return joined;
            }
        }
        PathBuf::from(path)
    }

    /// Returns the config file path in this directory.
    pub fn get_config_file_path(&self) -> PathBuf {
        self.expanded_path.join("config.toml")
    }

    fn write_default_config_file(&self) -> anyhow::Result<()> {
        let config_string = ConfigToml::default_string();
        let config_file_path = self.get_config_file_path();
        let mut config_file = std::fs::File::create(config_file_path)?;
        config_file.write_all(config_string.as_bytes())?;
        Ok(())
    }

    /// Returns the path to the secret file.
    pub fn get_secret_file_path(&self) -> PathBuf {
        self.expanded_path.join("secret")
    }
}

impl DataDirTrait for DataDir {
    /// Returns the full path to the data directory.
    fn path(&self) -> &Path {
        &self.expanded_path
    }

    /// Makes sure the data directory exists.
    /// Create the directory if it doesn't exist.
    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.expanded_path)?;

        // Check if we can write to the data directory
        let test_file_path = self
            .expanded_path
            .join("test_write_f2d560932f9b437fa9ef430ba436d611"); // random file name to not conflict with anything
        std::fs::write(test_file_path.clone(), b"test")
            .map_err(|err| anyhow::anyhow!("Failed to write to data directory: {}", err))?;
        std::fs::remove_file(test_file_path)
            .map_err(|err| anyhow::anyhow!("Failed to write to data directory: {}", err))?;
        Ok(())
    }

    /// Reads the config file from the data directory.
    /// Creates a default config file if it doesn't exist.
    fn read_or_create_config_file(&self) -> anyhow::Result<ConfigToml> {
        let config_file_path = self.get_config_file_path();
        if !config_file_path.exists() {
            self.write_default_config_file()?;
        }
        let config = ConfigToml::from_file(config_file_path)?;
        Ok(config)
    }

    /// Reads the secret file. Creates a new secret file if it doesn't exist.
    fn read_or_create_keypair(&self) -> anyhow::Result<pkarr::Keypair> {
        let secret_file_path = self.get_secret_file_path();
        if !secret_file_path.exists() {
            // Create a new secret file
            let keypair = pkarr::Keypair::random();
            let secret = keypair.secret_key();
            let hex_string = hex::encode(secret);
            std::fs::write(secret_file_path.clone(), hex_string)?;
            std::fs::set_permissions(&secret_file_path, std::fs::Permissions::from_mode(0o600))?;
            tracing::info!("Secret file created at {}", secret_file_path.display());
        }
        // Read the secret file
        let secret = std::fs::read(secret_file_path)?;
        let secret_bytes = hex::decode(secret)?;
        let secret_bytes: [u8; 32] = secret_bytes.try_into().map_err(|_| {
            anyhow::anyhow!("Failed to convert secret bytes into array of length 32")
        })?;
        let keypair = pkarr::Keypair::from_secret_key(&secret_bytes);
        Ok(keypair)
    }
}

impl Default for DataDir {
    fn default() -> Self {
        Self::new(PathBuf::from("~/.pubky"))
    }
}

impl DataDir {
    /// Creates a new data directory in a temporary directory.
    /// The temporary directory will be cleaned up when the DataDir is dropped.
    #[cfg(any(test, feature = "testing"))]
    pub fn test() -> Self {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let mut dir = Self::new(PathBuf::from(temp_dir.path()));
        dir.temp_dir = Arc::new(Some(temp_dir));
        dir
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use tempfile::TempDir;

    /// Test that the home directory is expanded correctly.
    #[test]
    pub fn test_expand_home_dir() {
        let data_dir = DataDir::new(PathBuf::from("~/.pubky"));
        let homedir = dirs::home_dir().unwrap();
        let expanded_path = homedir.join(".pubky");
        assert_eq!(data_dir.expanded_path, expanded_path);
    }

    /// Test that the data directory is created if it doesn't exist.
    #[test]
    pub fn test_ensure_data_dir_exists_and_is_accessible() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());

        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();
        assert!(test_path.exists());
        // temp_dir will be automatically cleaned up when it goes out of scope
    }

    #[test]
    pub fn test_get_default_config_file_path_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();
        let config_file_path = data_dir.get_config_file_path();
        assert!(!config_file_path.exists()); // Should not exist yet

        let mut config_file = std::fs::File::create(config_file_path.clone()).unwrap();
        config_file.write_all(b"test").unwrap();
        assert!(config_file_path.exists()); // Should exist now
                                            // temp_dir will be automatically cleaned up when it goes out of scope
    }

    #[test]
    pub fn test_read_or_create_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();
        let _ = data_dir.read_or_create_config_file().unwrap(); // Should create a default config file
        assert!(data_dir.get_config_file_path().exists());

        let _ = data_dir.read_or_create_config_file().unwrap(); // Should read the existing file
        assert!(data_dir.get_config_file_path().exists());
    }

    #[test]
    pub fn test_read_or_create_config_file_dont_override_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();

        // Write a broken config file
        let config_file_path = data_dir.get_config_file_path();
        std::fs::write(config_file_path.clone(), b"test").unwrap();
        assert!(config_file_path.exists()); // Should exist now

        // Try to read the config file and fail because config is broken
        let read_result = data_dir.read_or_create_config_file();
        assert!(read_result.is_err());

        // Make sure the broken config file is still there
        let content = std::fs::read_to_string(config_file_path).unwrap();
        assert_eq!(content, "test");
    }

    #[test]
    pub fn test_create_secret_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();

        let _ = data_dir.read_or_create_keypair().unwrap();
        assert!(data_dir.get_secret_file_path().exists());
    }

    #[test]
    pub fn test_dont_override_existing_secret_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();

        // Create a secret file
        let secret_file_path = data_dir.get_secret_file_path();
        std::fs::write(secret_file_path.clone(), b"test").unwrap();

        let result = data_dir.read_or_create_keypair();
        assert!(result.is_err());
        assert!(data_dir.get_secret_file_path().exists());
        let content = std::fs::read_to_string(secret_file_path).unwrap();
        assert_eq!(content, "test");
    }
}
