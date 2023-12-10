//! Configuration for the Kytz CLI.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Name of directory that wraps all kytz files in a given application directory
const KYTZ_DIR: &str = "kytz";

/// Returns the path to the user's kytz config directory.
pub fn kytz_config_root() -> Result<PathBuf> {
    // if let Some(val) = env::var_os("IROH_CONFIG_DIR") {
    //     return Ok(PathBuf::from(val));
    // }
    let cfg = dirs_next::config_dir().ok_or_else(|| {
        Error::Generic("operating environment provides no directory for configuration".to_string())
    })?;
    Ok(cfg.join(KYTZ_DIR))
}

/// Path that leads to a file in the iroh config directory.
pub fn kytz_config_path(file_name: impl AsRef<Path>) -> Result<PathBuf> {
    let path = kytz_config_root()?.join(file_name);
    Ok(path)
}
