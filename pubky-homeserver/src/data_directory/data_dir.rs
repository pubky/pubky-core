use super::ConfigToml;
use std::path::Path;

/// The source from which the homeserver bootstraps its configuration and identity.
///
/// `DataDir` abstracts over how the server's initial state is obtained so
/// that the same startup code path works for both production (reading real files
/// from disk via [`crate::PersistentDataDir`]) and testing (supplying pre-built
/// values in memory via [`crate::MockDataDir`]).
pub trait DataDir: std::fmt::Debug + Send + Sync {
    /// Returns the path to the root data directory.
    fn path(&self) -> &Path;

    /// Ensures the data directory exists and is writable.
    ///
    /// Creates the directory hierarchy when it is absent and verifies that
    /// the server process can write to it before any startup I/O is attempted.
    fn ensure_data_dir_exists_and_is_writable(&self) -> anyhow::Result<()>;

    /// Reads the configuration from the source, or creates a default config if it doesn't exist.
    fn read_or_create_config_file(&self) -> anyhow::Result<ConfigToml>;

    /// Reads the secret file from the data directory.
    /// Creates a new secret file if it doesn't exist.
    fn read_or_create_keypair(&self) -> anyhow::Result<pubky_common::crypto::Keypair>;
}
