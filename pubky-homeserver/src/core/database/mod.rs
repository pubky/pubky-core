//! Internal database in [crate::HomeserverCore]

use std::{fs, path::PathBuf};

use heed::{Env, EnvOpenOptions};

mod migrations;
pub mod tables;

use tables::{Tables, TABLES_COUNT};

pub use protected::DB;

/// Protecting fields from being mutated by modules in crate::database
mod protected {

    use crate::core::CoreConfig;

    use super::*;

    #[derive(Debug, Clone)]
    pub struct DB {
        pub(crate) env: Env,
        pub(crate) tables: Tables,
        pub(crate) buffers_dir: PathBuf,
        pub(crate) max_chunk_size: usize,
        config: CoreConfig,
    }

    impl DB {
        /// # Safety
        /// DB uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
        /// because the possible Undefined Behavior (UB) if the lock file is broken.
        pub unsafe fn open(config: CoreConfig) -> anyhow::Result<Self> {
            let buffers_dir = config.storage.clone().join("buffers");

            // Cleanup buffers.
            let _ = fs::remove_dir(&buffers_dir);
            fs::create_dir_all(&buffers_dir)?;

            let env = unsafe {
                EnvOpenOptions::new()
                    .max_dbs(TABLES_COUNT)
                    .map_size(config.db_map_size)
                    .open(&config.storage)
            }?;

            let tables = migrations::run(&env)?;

            let db = DB {
                env,
                tables,
                config,
                buffers_dir,
                max_chunk_size: max_chunk_size(),
            };

            Ok(db)
        }

        // Create an ephemeral database for testing purposes.
        pub fn test() -> DB {
            unsafe { DB::open(CoreConfig::test()).unwrap() }
        }

        // === Getters ===

        pub fn config(&self) -> &CoreConfig {
            &self.config
        }
    }
}

/// calculate optimal chunk size:
/// - https://lmdb.readthedocs.io/en/release/#storage-efficiency-limits
/// - https://github.com/lmdbjava/benchmarks/blob/master/results/20160710/README.md#test-2-determine-24816-kb-byte-values
fn max_chunk_size() -> usize {
    let page_size = page_size::get();

    // - 16 bytes Header  per page (LMDB)
    // - Each page has to contain 2 records
    // - 8 bytes per record (LMDB) (empirically, it seems to be 10 not 8)
    // - 12 bytes key:
    //      - timestamp : 8 bytes
    //      - chunk index: 4 bytes
    ((page_size - 16) / 2) - (8 + 2) - 12
}
