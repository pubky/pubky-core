use std::path::PathBuf;

use anyhow::Result;

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{app_cache_dir, app_files_dir};

/// Compute the default data directory used by the homeserver.
///
/// On mobile platforms like Android we need to store data in the app's
/// sandboxed storage. Desktop platforms continue to use the user's home
/// directory.
pub fn default_data_dir_path() -> PathBuf {
    #[cfg(target_os = "android")]
    {
        match app_files_dir() {
            Ok(mut dir) => {
                dir.push("pubky");
                return dir;
            }
            Err(error) => {
                tracing::warn!(?error, "Falling back to home directory for data storage");
            }
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pubky")
}

/// Create a temporary directory that is valid for the current platform.
///
/// Android applications cannot access the global `/tmp` directory, so tests and
/// helper utilities must place their temporary data inside the app's sandbox.
/// Desktop platforms keep using the OS default temporary location.
pub fn create_temp_dir(prefix: &str) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);

    #[cfg(target_os = "android")]
    {
        match app_cache_dir() {
            Ok(base) => {
                std::fs::create_dir_all(&base)?;
                return Ok(builder.tempdir_in(base)?);
            }
            Err(error) => {
                tracing::warn!(?error, "Falling back to default temporary directory");
            }
        }
    }

    Ok(builder.tempdir()?)
}
