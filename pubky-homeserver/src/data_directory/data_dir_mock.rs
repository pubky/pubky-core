use std::path::Path;

use super::DataDirTrait;

/// Mock data directory for testing.
/// 
/// It uses a temporary directory to store all data in. The data is removed as soon as the object is dropped.
/// 
#[derive(Debug, Clone)]
pub struct DataDirMock {
    pub(crate) temp_dir: std::sync::Arc<tempfile::TempDir>,
    pub(crate) config_toml: super::ConfigToml,
    pub(crate) keypair: pkarr::Keypair,
}

impl DataDirMock {
    /// Create a new DataDirMock with a temporary directory.
    pub fn new(config_toml: super::ConfigToml, keypair: pkarr::Keypair) -> anyhow::Result<Self> {
        Ok(Self { temp_dir: std::sync::Arc::new(tempfile::TempDir::new()?), config_toml, keypair })
    }
}

impl DataDirTrait for DataDirMock {
    fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        Ok(()) // Always ok because this is validated by the tempfile crate.
    }
    
    fn read_or_create_config_file(&self) -> anyhow::Result<super::ConfigToml> {
        Ok(self.config_toml.clone())
    }
    
    fn read_or_create_keypair(&self) -> anyhow::Result<pkarr::Keypair> {
        Ok(self.keypair.clone())
    }
}