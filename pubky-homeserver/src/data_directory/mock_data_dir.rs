use std::path::Path;

use crate::data_directory::config_toml::DEFAULT_TOS;

use super::DataDir;

/// Mock data directory for testing.
///
/// It uses a temporary directory to store all data in. The data is removed as soon as the object is dropped.
///

#[derive(Debug, Clone)]
pub struct MockDataDir {
    pub(crate) temp_dir: std::sync::Arc<tempfile::TempDir>,
    /// The configuration for the homeserver.
    pub config_toml: super::ConfigToml,
    /// The keypair for the homeserver.
    pub keypair: pkarr::Keypair,
}

impl MockDataDir {
    /// Create a new DataDirMock with a temporary directory.
    ///
    /// If keypair is not provided, a new one will be generated.
    pub fn new(
        config_toml: super::ConfigToml,
        keypair: Option<pkarr::Keypair>,
    ) -> anyhow::Result<Self> {
        let keypair = keypair.unwrap_or_else(pkarr::Keypair::random);
        Ok(Self {
            temp_dir: std::sync::Arc::new(tempfile::TempDir::new()?),
            config_toml,
            keypair,
        })
    }

    /// Creates a mock data directory with a config and keypair appropriate for testing.
    #[cfg(any(test, feature = "testing"))]
    pub fn test() -> Self {
        let config = super::ConfigToml::test();
        let keypair = pkarr::Keypair::from_secret_key(&[0; 32]);
        Self::new(config, Some(keypair)).expect("failed to create MockDataDir")
    }
}

impl Default for MockDataDir {
    fn default() -> Self {
        Self::test()
    }
}

impl DataDir for MockDataDir {
    fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        Ok(()) // Always ok because this is validated by the tempfile crate.
    }

    fn ensure_tos_file_exists_if_enforced(&self, config: &super::ConfigToml) -> anyhow::Result<()> {
        if config.general.enforce_tos {
            let tos_path = self.path().join("tos.html");
            if !tos_path.exists() {
                std::fs::write(tos_path, DEFAULT_TOS)?;
            }
        }
        Ok(())
    }

    fn read_or_create_config_file(&self) -> anyhow::Result<super::ConfigToml> {
        Ok(self.config_toml.clone())
    }

    fn read_or_create_keypair(&self) -> anyhow::Result<pkarr::Keypair> {
        Ok(self.keypair.clone())
    }
}
