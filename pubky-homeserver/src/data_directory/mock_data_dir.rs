use super::DataDir;
use crate::storage_config::StorageConfigToml;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Mock data directory for testing.
///
/// By default it uses a temporary directory that is removed when the value is dropped.
/// Use [`MockDataDir::new_persistent_data_dir`] to point at a real directory that
/// survives across restarts.
#[derive(Debug, Clone)]
pub struct MockDataDir {
    root: MockDataDirKind,
    /// The configuration for the homeserver.
    pub config_toml: super::ConfigToml,
    /// The keypair for the homeserver.
    pub keypair: pubky_common::crypto::Keypair,
}

impl MockDataDir {
    /// Create a new [`MockDataDir`] with an ephemeral temporary directory.
    ///
    /// If keypair is not provided, a new one will be generated.
    pub fn new(
        config_toml: super::ConfigToml,
        keypair: Option<pubky_common::crypto::Keypair>,
    ) -> anyhow::Result<Self> {
        let keypair = keypair.unwrap_or_else(pubky_common::crypto::Keypair::random);

        Ok(Self {
            root: MockDataDirKind::Temp(std::sync::Arc::new(tempfile::TempDir::new()?)),
            config_toml,
            keypair,
        })
    }

    /// Create a [`MockDataDir`] backed by an existing (persistent) directory.
    ///
    /// The directory is **not** cleaned up on drop. Use this when you need
    /// data to survive across process restarts (e.g. integration tests that
    /// verify persistence).
    pub fn new_persistent_data_dir(
        data_dir: PathBuf,
        config_toml: super::ConfigToml,
        keypair: Option<pubky_common::crypto::Keypair>,
    ) -> anyhow::Result<Self> {
        let keypair = keypair.unwrap_or_else(pubky_common::crypto::Keypair::random);
        std::fs::create_dir_all(&data_dir)?;

        debug_assert!(
            matches!(config_toml.storage, StorageConfigToml::FileSystem),
            "MockDataDir with persistent data directory should use FileSystem storage config"
        );

        Ok(Self {
            root: MockDataDirKind::Persistent(data_dir),
            config_toml,
            keypair,
        })
    }

    /// Returns true if the root of this [`MockDataDir`] is a temporary directory.
    pub fn is_temp(&self) -> bool {
        matches!(self.root, MockDataDirKind::Temp(_))
    }

    /// Creates a mock data directory with a config and keypair appropriate for testing.
    ///
    /// Uses [`ConfigToml::default_test_config()`] which enables the admin server.
    /// For lightweight tests, use [`MockDataDir::new()`] with [`ConfigToml::minimal_test_config()`].
    #[cfg(any(test, feature = "testing"))]
    pub fn test() -> Self {
        let config = super::ConfigToml::default_test_config();
        let keypair = pubky_common::crypto::Keypair::from_secret(&[0; 32]);

        Self::new(config, Some(keypair)).expect("failed to create MockDataDir")
    }

    /// Creates a [`MockDataDir`] with a config and keypair for testing, backed by a real directory.
    ///
    /// Same as [`MockDataDir::test()`] but with a real directory that is not cleaned up on drop.
    /// Use this for integration tests that need to verify persistence across process restarts.
    #[cfg(any(test, feature = "testing"))]
    pub fn test_persistent_data_dir(data_dir: PathBuf) -> Self {
        let mut config = super::ConfigToml::default_test_config();
        // Set storage to `FileSystem` for persistent data directory
        config.storage = StorageConfigToml::FileSystem;
        let keypair = pubky_common::crypto::Keypair::from_secret(&[0; 32]);

        Self::new_persistent_data_dir(data_dir, config, Some(keypair))
            .expect("failed to create MockDataDir")
    }
}

impl Default for MockDataDir {
    fn default() -> Self {
        Self::test()
    }
}

impl DataDir for MockDataDir {
    fn path(&self) -> &Path {
        match &self.root {
            MockDataDirKind::Temp(temp_dir) => temp_dir.path(),
            MockDataDirKind::Persistent(path) => path.as_path(),
        }
    }

    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()> {
        match &self.root {
            MockDataDirKind::Temp(_) => {
                // Always ok because this is validated by the tempfile crate.
                Ok(())
            }
            MockDataDirKind::Persistent(path) => {
                std::fs::create_dir_all(path)?;

                // Check if we can write to the data directory
                let test_file_path = path.join(format!("test_write_{}", Uuid::new_v4().simple()));
                std::fs::write(test_file_path.clone(), b"test")
                    .map_err(|err| anyhow::anyhow!("Failed to write to data directory: {err}"))?;
                std::fs::remove_file(test_file_path)
                    .map_err(|err| anyhow::anyhow!("Failed to remove from data directory: {err}"))?;

                Ok(())
            }
        }
    }

    fn read_or_create_config_file(&self) -> anyhow::Result<super::ConfigToml> {
        Ok(self.config_toml.clone())
    }

    fn read_or_create_keypair(&self) -> anyhow::Result<pubky_common::crypto::Keypair> {
        Ok(self.keypair.clone())
    }
}

/// Backing storage for a [`MockDataDir`]: either a temporary directory that is
/// cleaned up on drop, or a caller-supplied persistent path.
#[derive(Debug, Clone)]
enum MockDataDirKind {
    Temp(std::sync::Arc<tempfile::TempDir>),
    Persistent(PathBuf),
}
