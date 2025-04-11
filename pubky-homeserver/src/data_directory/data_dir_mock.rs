use std::path::Path;

use super::DataDirTrait;

/// Mock data directory for testing.
///
/// It uses a temporary directory to store all data in. The data is removed as soon as the object is dropped.
///

#[derive(Debug, Clone)]
pub struct DataDirMock {
    pub(crate) temp_dir: std::sync::Arc<tempfile::TempDir>,
    /// The configuration for the homeserver.
    pub config_toml: super::ConfigToml,
    /// The keypair for the homeserver.
    pub keypair: pkarr::Keypair,
}

impl DataDirMock {
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
    pub fn test() -> Self {
        let config = super::ConfigToml::test();
        let keypair = pkarr::Keypair::from_secret_key(&[0; 32]);
        Self::new(config, Some(keypair)).unwrap()
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
