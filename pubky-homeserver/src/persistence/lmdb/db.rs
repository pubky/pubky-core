use super::tables::{Tables, TABLES_COUNT};
use heed::{Env, EnvOpenOptions};
use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::migrations;

pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

#[derive(Debug, Clone)]
pub struct LmDB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
    // Only used for testing purposes to keep the testdir alive.
    #[allow(dead_code)]
    test_dir: Option<Arc<tempfile::TempDir>>,
}

impl LmDB {
    /// # Safety
    /// DB uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub unsafe fn open(main_dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(main_dir)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(TABLES_COUNT)
                .map_size(DEFAULT_MAP_SIZE)
                .open(main_dir)
        }?;

        migrations::run(&env)?;
        let mut wtxn = env.write_txn()?;
        let tables = Tables::new(&env, &mut wtxn)?;
        wtxn.commit()?;

        let db = LmDB {
            env,
            tables,
            test_dir: None,
        };

        Ok(db)
    }

    // Create an ephemeral database for testing purposes.
    #[cfg(test)]
    pub fn test() -> LmDB {
        // Create a temporary directory for the test.
        let temp_dir = tempfile::tempdir().unwrap();
        let mut lmdb = unsafe { LmDB::open(temp_dir.path()).unwrap() };
        lmdb.test_dir = Some(Arc::new(temp_dir)); // Keep the directory alive for the duration of the test. As soon as all LmDB instances are dropped, the directory will be deleted automatically.

        lmdb
    }
}
