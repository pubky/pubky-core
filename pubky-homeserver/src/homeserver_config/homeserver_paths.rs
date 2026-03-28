//! Paths for the homeserver's important files and directories needed for configuration of the server and its storage backends.

use super::{setup_source::SetupSource, ConfigToml};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Paths for the homeserver's important files and directories.
#[derive(Debug, Clone)]
pub struct HomeserverPaths {
    /// Root data directory.
    data_dir: PathBuf,
    /// Resolved config file path.
    config_file_path: PathBuf,
    /// Resolved secret key file path.
    secret_file_path: PathBuf,
}

impl HomeserverPaths {
    /// Creates a new `HomeserverPaths` with all paths derived from `path`.
    ///
    /// `path` will be expanded if it starts with `~`.
    /// Config → `{path}/config.toml`, secret → `{path}/secret`.
    pub fn new(path: PathBuf) -> Self {
        let expanded = Self::expand_home_dir(path);
        let config_file_path = expanded.join("config.toml");
        let secret_file_path = expanded.join("secret");

        Self {
            data_dir: expanded,
            config_file_path,
            secret_file_path,
        }
    }

    /// Creates a `HomeserverPaths` with independent overrides for the config
    /// file and secret key file.
    ///
    /// Resolution rules (highest priority first):
    /// 1. Explicit `config_file` / `secret_file` argument.
    /// 2. Derived from `data_dir` (`{data_dir}/config.toml`, `{data_dir}/secret`).
    /// 3. `data_dir` itself defaults to `~/.pubky` when built from CLI defaults.
    ///
    /// This allows full separation of config, secret, and data storage
    /// locations, which is required for production deployments.
    pub fn new_with_overrides(
        data_dir: PathBuf,
        config_file: Option<PathBuf>,
        secret_file: Option<PathBuf>,
    ) -> Self {
        let expanded = Self::expand_home_dir(data_dir);
        let config_file_path = config_file
            .map(Self::expand_home_dir)
            .unwrap_or_else(|| expanded.join("config.toml"));
        let secret_file_path = secret_file
            .map(Self::expand_home_dir)
            .unwrap_or_else(|| expanded.join("secret"));

        Self {
            data_dir: expanded,
            config_file_path,
            secret_file_path,
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

    fn write_sample_config_file(&self) -> anyhow::Result<()> {
        let config_string = ConfigToml::sample_string();
        let mut config_file = std::fs::File::create(&self.config_file_path)?;
        config_file.write_all(config_string.as_bytes())?;

        Ok(())
    }
}

impl HomeserverPaths {
    /// Returns the resolved config file path.
    pub fn config_file_path(&self) -> &PathBuf {
        &self.config_file_path
    }

    /// Returns the resolved secret key file path.
    pub fn secret_file_path(&self) -> &PathBuf {
        &self.secret_file_path
    }
}

impl SetupSource for HomeserverPaths {
    /// Returns the full path to the data directory.
    fn data_dir_path(&self) -> &Path {
        &self.data_dir
    }

    /// Makes sure the data directory exists.
    /// Create the directory if it doesn't exist.
    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;

        // Check if we can write to the data directory
        let test_file_path = self
            .data_dir
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
        if !self.config_file_path.exists() {
            self.write_sample_config_file()?;
        }

        let config = ConfigToml::from_file(&self.config_file_path)?;

        Ok(config)
    }

    /// Reads the secret file. Creates a new secret file if it doesn't exist.
    fn read_or_create_keypair(&self) -> anyhow::Result<pubky_common::crypto::Keypair> {
        if !self.secret_file_path.exists() {
            // Create a new secret file
            pubky_common::crypto::Keypair::random()
                .write_secret_key_file(&self.secret_file_path)?;
            tracing::info!("Secret file created at {}", self.secret_file_path.display());
        }

        // Read the secret file
        let keypair = pubky_common::crypto::Keypair::from_secret_key_file(&self.secret_file_path)?;

        Ok(keypair)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use tempfile::TempDir;

    #[test]
    pub fn test_expand_home_dir() {
        let paths = HomeserverPaths::new(PathBuf::from("~/.pubky"));
        let homedir = dirs::home_dir().unwrap();
        let expanded_path = homedir.join(".pubky");
        assert_eq!(paths.data_dir, expanded_path);
    }

    #[test]
    pub fn test_ensure_data_dir_exists_and_is_accessible() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());

        paths.ensure_data_dir_exists_and_is_writable().unwrap();
        assert!(test_path.exists());
    }

    #[test]
    pub fn test_get_default_config_file_path_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();
        let config_file_path = paths.config_file_path();
        assert!(!config_file_path.exists()); // Should not exist yet

        let mut config_file = std::fs::File::create(config_file_path).unwrap();
        config_file.write_all(b"test").unwrap();
        assert!(config_file_path.exists()); // Should exist now
    }

    #[test]
    pub fn test_read_or_create_config_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();
        let _ = paths.read_or_create_config_file().unwrap(); // Should create a default config file
        assert!(paths.config_file_path().exists());

        let _ = paths.read_or_create_config_file().unwrap(); // Should read the existing file
        assert!(paths.config_file_path().exists());
    }

    #[test]
    pub fn test_read_or_create_config_file_dont_override_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        // Write a broken config file
        let config_file_path = paths.config_file_path();
        std::fs::write(config_file_path, b"test").unwrap();
        assert!(config_file_path.exists()); // Should exist now

        // Try to read the config file and fail because config is broken
        let read_result = paths.read_or_create_config_file();
        assert!(read_result.is_err());

        // Make sure the broken config file is still there
        let content = std::fs::read_to_string(config_file_path).unwrap();
        assert_eq!(content, "test");
    }

    #[test]
    pub fn test_create_secret_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        let _ = paths.read_or_create_keypair().unwrap();
        assert!(paths.secret_file_path().exists());
    }

    #[test]
    pub fn test_dont_override_existing_secret_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        // Create a secret file
        let secret_file_path = paths.secret_file_path();
        std::fs::write(secret_file_path, b"test").unwrap();

        let result = paths.read_or_create_keypair();
        assert!(result.is_err());
        assert!(paths.secret_file_path().exists());
        let content = std::fs::read_to_string(secret_file_path).unwrap();
        assert_eq!(content, "test");
    }

    #[test]
    pub fn test_trim_secret_file_content() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let paths = HomeserverPaths::new(test_path.clone());
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        // Create a secret file
        let keypair = pubky_common::crypto::Keypair::random();
        let secret_file_path = paths.secret_file_path();
        let file_content = format!("\n {}\n \n", hex::encode(keypair.secret_key()));
        std::fs::write(secret_file_path, file_content).unwrap();

        let result = paths.read_or_create_keypair();
        assert!(result.is_ok());
        let read_keypair = result.unwrap();
        assert_eq!(read_keypair.secret_key(), keypair.secret_key());
    }

    #[test]
    fn test_new_with_overrides_none_falls_back_to_default_derivation() {
        let temp_dir = TempDir::new().unwrap();
        let paths = HomeserverPaths::new_with_overrides(temp_dir.path().to_path_buf(), None, None);
        assert_eq!(
            paths.config_file_path(),
            &temp_dir.path().join("config.toml")
        );
        assert_eq!(paths.secret_file_path(), &temp_dir.path().join("secret"));
    }

    #[test]
    fn test_new_with_overrides_explicit_config_file_is_used() {
        let temp_dir = TempDir::new().unwrap();
        let custom_config = PathBuf::from("/etc/pubky/config.toml");
        let paths = HomeserverPaths::new_with_overrides(
            temp_dir.path().to_path_buf(),
            Some(custom_config.clone()),
            None,
        );
        assert_eq!(paths.config_file_path(), &custom_config);
        // Secret should still fall back to data_dir
        assert_eq!(paths.secret_file_path(), &temp_dir.path().join("secret"));
    }

    #[test]
    fn test_new_with_overrides_explicit_secret_file_is_used() {
        let temp_dir = TempDir::new().unwrap();
        let custom_secret = PathBuf::from("/run/secrets/pubky_secret");
        let paths = HomeserverPaths::new_with_overrides(
            temp_dir.path().to_path_buf(),
            None,
            Some(custom_secret.clone()),
        );
        // Config should still fall back to data_dir
        assert_eq!(
            paths.config_file_path(),
            &temp_dir.path().join("config.toml")
        );
        assert_eq!(paths.secret_file_path(), &custom_secret);
    }

    #[test]
    fn test_new_with_overrides_full_override_both_files() {
        let temp_dir = TempDir::new().unwrap();
        let custom_config = temp_dir.path().join("my.toml");
        let custom_secret = temp_dir.path().join("my.secret");
        let paths = HomeserverPaths::new_with_overrides(
            temp_dir.path().to_path_buf(),
            Some(custom_config.clone()),
            Some(custom_secret.clone()),
        );
        assert_eq!(paths.config_file_path(), &custom_config);
        assert_eq!(paths.secret_file_path(), &custom_secret);
    }

    #[test]
    fn test_new_with_overrides_read_or_create_config_uses_override_path() {
        let temp_dir = TempDir::new().unwrap();
        // Put the config file in a separate subdirectory to verify it's read from there.
        let config_dir = temp_dir.path().join("cfg");
        std::fs::create_dir_all(&config_dir).unwrap();
        let config_file = config_dir.join("config.toml");

        let data_dir_root = temp_dir.path().join("data");
        let paths = HomeserverPaths::new_with_overrides(
            data_dir_root.clone(),
            Some(config_file.clone()),
            None,
        );
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        // Config file does not exist yet → should be created at the override path.
        paths.read_or_create_config_file().unwrap();
        assert!(config_file.exists(), "config created at override path");
        // Default derivation path should NOT have been created.
        assert!(
            !data_dir_root.join("config.toml").exists(),
            "default path untouched"
        );
    }

    #[test]
    fn test_new_with_overrides_read_or_create_keypair_uses_override_path() {
        let temp_dir = TempDir::new().unwrap();
        let secret_dir = temp_dir.path().join("secrets");
        std::fs::create_dir_all(&secret_dir).unwrap();
        let secret_file = secret_dir.join("homeserver.secret");

        let data_dir_root = temp_dir.path().join("data");
        let paths = HomeserverPaths::new_with_overrides(
            data_dir_root.clone(),
            None,
            Some(secret_file.clone()),
        );
        paths.ensure_data_dir_exists_and_is_writable().unwrap();

        paths.read_or_create_keypair().unwrap();
        assert!(secret_file.exists(), "secret created at override path");
        assert!(
            !data_dir_root.join("secret").exists(),
            "default path untouched"
        );
    }
}
