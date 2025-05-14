use super::tables::{Tables, TABLES_COUNT};
use heed::{Env, EnvOpenOptions};
use std::sync::Arc;
use std::{fs, path::PathBuf};

use super::migrations;

pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

#[derive(Debug, Clone)]
pub struct LmDB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
    pub(crate) buffers_dir: PathBuf,
    pub(crate) max_chunk_size: usize,
    // Only used for testing purposes to keep the testdir alive.
    #[allow(dead_code)]
    test_dir: Option<Arc<tempfile::TempDir>>,
}

impl LmDB {
    /// # Safety
    /// DB uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    pub unsafe fn open(main_dir: PathBuf) -> anyhow::Result<Self> {
        let buffers_dir = main_dir.join("buffers");

        // Cleanup buffers.
        let _ = fs::remove_dir(&buffers_dir);
        fs::create_dir_all(&buffers_dir)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(TABLES_COUNT)
                .map_size(DEFAULT_MAP_SIZE)
                .open(&main_dir)
        }?;

        migrations::run(&env)?;
        let mut wtxn = env.write_txn()?;
        let tables = Tables::new(&env, &mut wtxn)?;
        wtxn.commit()?;

        let db = LmDB {
            env,
            tables,
            buffers_dir,
            max_chunk_size: Self::max_chunk_size(),
            test_dir: None,
        };

        Ok(db)
    }

    // Create an ephemeral database for testing purposes.
    #[cfg(test)]
    pub fn test() -> LmDB {
        // Create a temporary directory for the test.
        let temp_dir = tempfile::tempdir().unwrap();
        let mut lmdb = unsafe { LmDB::open(PathBuf::from(temp_dir.path())).unwrap() };
        lmdb.test_dir = Some(Arc::new(temp_dir)); // Keep the directory alive for the duration of the test. As soon as all LmDB instances are dropped, the directory will be deleted automatically.

        lmdb
    }

    /// calculate optimal chunk size:
    /// - <https://lmdb.readthedocs.io/en/release/#storage-efficiency-limits>
    /// - <https://github.com/lmdbjava/benchmarks/blob/master/results/20160710/README.md#test-2-determine-24816-kb-byte-values>
    pub fn max_chunk_size() -> usize {
        let page_size = page_size::get();

        // - 16 bytes Header  per page (LMDB)
        // - Each page has to contain 2 records
        // - 8 bytes per record (LMDB) (empirically, it seems to be 10 not 8)
        // - 12 bytes key:
        //      - timestamp : 8 bytes
        //      - chunk index: 4 bytes
        ((page_size - 16) / 2) - (8 + 2) - 12
    }
}
