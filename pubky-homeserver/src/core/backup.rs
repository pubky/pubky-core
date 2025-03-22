use crate::core::database::DB;
use heed::CompactionOption;
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::{interval_at, Instant};
use tracing::{error, info};

const BACKUP_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60); // 4 hours

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
pub async fn backup_lmdb_periodically(db: DB, backup_path: PathBuf) {
    // Schedule the first backup after the interval.
    let start_time = Instant::now() + BACKUP_INTERVAL;
    let mut interval_timer = interval_at(start_time, BACKUP_INTERVAL);

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
        .expect("Failed to execute backup task");
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
fn do_backup(db: DB, backup_path: PathBuf) {
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
    use std::fs;
    use tempfile::tempdir;

    /// Tests that the backup creates the final backup file with the `.mdb` extension
    /// and that no temporary `.tmp` file is left after the backup process.
    #[test]
    fn test_do_backup_creates_backup_file() {
        // Create a test DB instance.
        let db = DB::test();

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

        // Clean up the final backup file.
        fs::remove_file(final_backup_file).expect("Failed to remove backup file");
    }
}
