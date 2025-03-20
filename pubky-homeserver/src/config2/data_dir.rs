use std::path::PathBuf;

use super::ConfigToml;


/// The data directory for the homeserver.
/// 
/// This is the directory that will store the homeserver's data.
/// 
/// It will be expanded to the home directory if it starts with "~".
///
#[derive(Debug, Clone)]
pub struct DataDir {
    expanded_path: PathBuf,
}

impl DataDir {
    /// Creates a new data directory.
    pub fn new(path: PathBuf) -> Self {
        Self { expanded_path: Self::expand_home_dir(path) }
    }

    /// Expands the data directory to the home directory if it starts with "~".
    /// Return the full path to the data directory.
    fn expand_home_dir(path: PathBuf) -> PathBuf {
        let path = match path.to_str() {
            Some(path) => path,
            None => {
                // Path not valid utf-8 so we can't expand it.
                return path
            },
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

    /// Makes sure the data directory exists.
    /// Create the directory if it doesn't exist.
    pub fn ensure_data_dir_exists_and_is_accessible(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.expanded_path)?;
        Ok(())
    }

    /// Returns the config file path in this directory.
    pub fn get_config_file_path(&self) -> PathBuf {
        self.expanded_path.join("config.toml")
    }

    /// Reads the config file from the data directory.
    pub fn read_config_file(&self) -> anyhow::Result<ConfigToml> {
        let config_file_path = self.get_config_file_path();
        let config = ConfigToml::from_file(config_file_path)?;
        Ok(config)
    }

    pub fn create_config_file(&self, config: &ConfigToml) -> anyhow::Result<()> {
        let config_file_path = self.get_config_file_path();
        let config_file = std::fs::File::create(config_file_path)?;
        config_file.write_all(b"test")?;
        Ok(())
    }


}

impl Default for DataDir {
    fn default() -> Self {
        Self::new(PathBuf::from("~/.pubky"))
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
        
        data_dir.ensure_data_dir_exists_and_is_accessible().unwrap();
        assert!(test_path.exists());
        // temp_dir will be automatically cleaned up when it goes out of scope
    }

    #[test]
    pub fn test_get_default_config_file_path_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_path = temp_dir.path().join(".pubky");
        let data_dir = DataDir::new(test_path.clone());
        data_dir.ensure_data_dir_exists_and_is_accessible().unwrap();
        let config_file_path = data_dir.get_config_file_path();
        assert!(!config_file_path.exists()); // Should not exist yet

        let mut config_file = std::fs::File::create(config_file_path.clone()).unwrap();
        config_file.write_all(b"test").unwrap();
        assert!(config_file_path.exists()); // Should exist now
        // temp_dir will be automatically cleaned up when it goes out of scope
    }
}
