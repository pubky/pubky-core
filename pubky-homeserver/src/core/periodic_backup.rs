use crate::{app_context::AppContext, persistence::lmdb::LmDB};
use heed::CompactionOption;
use std::path::PathBuf;
use std::time::Duration;
use tokio::{task::JoinHandle, time::interval};
use tracing::{error, info};

pub(crate) struct PeriodicBackup {
    handle: Option<JoinHandle<()>>,
}

const BACKUP_INTERVAL_DANGERZONE: Duration = Duration::from_secs(30);

impl PeriodicBackup {
    pub fn start(context: &AppContext) -> Self {
        let backup_interval =
            Duration::from_secs(context.config_toml.general.lmdb_backup_interval_s);
        let is_disabled = backup_interval.as_secs() == 0;
        if is_disabled {
            tracing::info!("LMDB backup is disabled.");
            return Self { handle: None };
        }
        if backup_interval < BACKUP_INTERVAL_DANGERZONE {
            tracing::warn!(
                "The configured LMDB backup interval is less than {}s!.",
                BACKUP_INTERVAL_DANGERZONE.as_secs(),
            );
        }
        let db = context.db.clone();
        let backup_path = context.data_dir.path().join("backup");
        tracing::info!(
            "Starting LMDB backup with interval {}s",
            backup_interval.as_secs()
        );
        let handle = tokio::spawn(async move {
            backup_lmdb_periodically(db, backup_path, backup_interval).await;
        });
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for PeriodicBackup {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

/// Periodically creates a backup of the LMDB environment every 4 hours.
///
/// The backup process performs the following steps:
/// 1. Copies the LMDB environment to a temporary file (with a `.tmp` extension),
///    ensuring it’s safe for moving or copying.
/// 2. Atomically renames the temporary file to a final backup file (with a `.mdb` extension).
///
/// # Arguments
///
/// * `db` - The LMDB database handle.
/// * `backup_path` - The base path for the backup file (extensions will be appended).
pub async fn backup_lmdb_periodically(db: LmDB, backup_path: PathBuf, period: Duration) {
    let mut interval_timer = interval(period);

    interval_timer.tick().await; // Ignore the first tick as it is instant.

    loop {
        // Wait for the next backup tick.
        interval_timer.tick().await;

        // Clone the database handle and backup path for use in the blocking task.
        let db_clone = db.clone();
        let backup_path_clone = backup_path.clone();

        // Execute the backup operation in a blocking task.
        tokio::task::spawn_blocking(move || {
            do_backup(db_clone, backup_path_clone);
        })
        .await
        .map_err(|e| error!("Backup task panicked: {:?}", e))
        .ok();
    }
}

/// Performs the actual backup of the LMDB environment.
///
/// It first writes the backup to a temporary file and, upon success, renames it
/// to the final backup file. Any errors encountered during these operations are logged.
///
/// # Arguments
///
/// * `db` - The LMDB database handle.
/// * `backup_path` - The base path for the backup file (extensions will be appended).
fn do_backup(db: LmDB, backup_path: PathBuf) {
    // Define file paths for the temporary and final backup files.
    let final_backup_path = backup_path.with_extension("mdb");
    let temp_backup_path = backup_path.with_extension("tmp");

    // Create a backup by copying the LMDB environment to the temporary file.
    if let Err(e) = db
        .env
        .copy_to_file(&temp_backup_path, CompactionOption::Enabled)
    {
        error!(
            "Failed to create temporary LMDB backup at {:?}: {:?}",
            temp_backup_path, e
        );
        return;
    }

    // Atomically rename the temporary file to the final backup file.
    if let Err(e) = std::fs::rename(&temp_backup_path, &final_backup_path) {
        error!(
            "Failed to rename temporary backup file {:?} to final backup file {:?}: {:?}",
            temp_backup_path, final_backup_path, e
        );
        return;
    }

    info!(
        "LMDB backup successfully created at {:?}",
        final_backup_path
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Tests that the backup creates the final backup file with the `.mdb` extension
    /// and that no temporary `.tmp` file is left after the backup process.
    #[test]
    fn test_do_backup_creates_backup_file() {
        // Create a test DB instance.
        let db = LmDB::test();

        // Create a temporary directory to store the backup.
        let temp_dir = tempdir().expect("Failed to create temporary directory");
        let backup_path = temp_dir.path().join("lmdb_backup");

        // Perform the backup.
        do_backup(db, backup_path.clone());

        // Define the expected final and temporary backup file paths.
        let final_backup_file = backup_path.with_extension("mdb");
        let temp_backup_file = backup_path.with_extension("tmp");

        // Assert the final backup file exists.
        assert!(
            final_backup_file.exists(),
            "Expected final backup file at {:?} to exist.",
            final_backup_file
        );

        // Assert that the temporary backup file was cleaned up.
        assert!(
            !temp_backup_file.exists(),
            "Expected temporary backup file at {:?} to be removed.",
            temp_backup_file
        );
    }
}
