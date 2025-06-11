use super::{data_dir::DataDir, ConfigToml};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// The data directory for the homeserver.
///
/// This is the directory that will store the homeservers data.
///
#[derive(Debug, Clone)]
pub struct PersistentDataDir {
    expanded_path: PathBuf,
}

impl PersistentDataDir {
    /// Creates a new data directory.
    /// `path` will be expanded to the home directory if it starts with "~".
    pub fn new(path: PathBuf) -> Self {
        Self {
            expanded_path: Self::expand_home_dir(path),
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

    fn write_sample_config_file(&self) -> anyhow::Result<()> {
        let config_string = ConfigToml::sample_string();
        let config_file_path = self.get_config_file_path();
        let mut config_file = std::fs::File::create(config_file_path)?;
        config_file.write_all(config_string.as_bytes())?;
        Ok(())
    }

    /// Returns the path to the secret file.
    pub fn get_secret_file_path(&self) -> PathBuf {
        self.expanded_path.join("secret")
    }

    /// Writes the keypair to the secret file.
    /// If the file already exists, it will be overwritten.
    pub fn write_keypair(&self, keypair: &pkarr::Keypair) -> anyhow::Result<()> {
        let secret_file_path = self.get_secret_file_path();
        let secret = keypair.secret_key();
        let hex_string = hex::encode(secret);
        std::fs::write(secret_file_path.clone(), hex_string)?;
        #[cfg(unix)]
        {
            std::fs::set_permissions(&secret_file_path, std::fs::Permissions::from_mode(0o600))?;
        }
        tracing::info!("Secret file created at {}", secret_file_path.display());
        Ok(())
    }
}

impl Default for PersistentDataDir {
    fn default() -> Self {
        Self::new(PathBuf::from("~/.pubky"))
    }
}

impl DataDir for PersistentDataDir {
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
            self.write_sample_config_file()?;
        }
        let config = ConfigToml::from_file(config_file_path)?;
        Ok(config)
    }

    /// Reads the secret file. Creates a new secret file if it doesn't exist.
    fn read_or_create_keypair(&self) -> anyhow::Result<pkarr::Keypair> {
        let secret_file_path = self.get_secret_file_path();
        if !secret_file_path.exists() {
            // Create a new secret file
            self.write_keypair(&pkarr::Keypair::random())?;
        }
        // Read the secret file
        let secret = std::fs::read(secret_file_path)?;
        let secret_string = String::from_utf8_lossy(&secret).trim().to_string();
        let secret_bytes = hex::decode(secret_string)?;
        let secret_bytes: [u8; 32] = secret_bytes.try_into().map_err(|_| {
            anyhow::anyhow!("Failed to convert secret bytes into array of length 32")
        })?;
        let keypair = pkarr::Keypair::from_secret_key(&secret_bytes);
        Ok(keypair)
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
        let data_dir = PersistentDataDir::new(PathBuf::from("~/.pubky"));
        let homedir = dirs::home_dir().unwrap();
        let expanded_path = homedir.join(".pubky");
        assert_eq!(data_dir.expanded_path, expanded_path);
    }

    /// Test that the data directory is created if it doesn't exist.
    #[test]
    pub fn test_ensure_data_dir_exists_and_is_accessible() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = PersistentDataDir::new(test_path.clone());

        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();
        assert!(test_path.exists());
        // temp_dir will be automatically cleaned up when it goes out of scope
    }

    #[test]
    pub fn test_get_default_config_file_path_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = PersistentDataDir::new(test_path.clone());
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
        let data_dir = PersistentDataDir::new(test_path.clone());
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
        let data_dir = PersistentDataDir::new(test_path.clone());
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
        let data_dir = PersistentDataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();

        let _ = data_dir.read_or_create_keypair().unwrap();
        assert!(data_dir.get_secret_file_path().exists());
    }

    #[test]
    pub fn test_dont_override_existing_secret_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = PersistentDataDir::new(test_path.clone());
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

    #[test]
    pub fn test_trim_secret_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = PersistentDataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_writable().unwrap();

        // Create a secret file
        let keypair = pkarr::Keypair::random();
        let secret_file_path = data_dir.get_secret_file_path();
        let file_content = format!("\n {}\n \n", hex::encode(keypair.secret_key()));
        std::fs::write(secret_file_path.clone(), file_content).unwrap();

        let result = data_dir.read_or_create_keypair();
        assert!(result.is_ok());
        let read_keypair = result.unwrap();
        assert_eq!(read_keypair.secret_key(), keypair.secret_key());
    }
}
