use std::{fs, path::PathBuf};

use heed::{Env, EnvOpenOptions};

mod migrations;
pub mod tables;

use crate::config::Config;

use tables::{Tables, TABLES_COUNT};

#[derive(Debug, Clone)]
pub struct DB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
    pub(crate) config: Config,
    pub(crate) buffers_dir: PathBuf,
    pub(crate) max_chunk_size: usize,
}

impl DB {
    pub fn open(config: Config) -> anyhow::Result<Self> {
        let buffers_dir = config.storage().clone().join("buffers");

        // Cleanup buffers.
        let _ = fs::remove_dir(&buffers_dir);
        fs::create_dir_all(&buffers_dir)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(TABLES_COUNT)
                .map_size(config.db_map_size())
                .open(config.storage())
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
}

/// calculate optimal chunk size:
/// - https://lmdb.readthedocs.io/en/release/#storage-efficiency-limits
/// - https://github.com/lmdbjava/benchmarks/blob/master/results/20160710/README.md#test-2-determine-24816-kb-byte-values
fn max_chunk_size() -> usize {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

    // - 16 bytes Header  per page (LMDB)
    // - Each page has to contain 2 records
    // - 8 bytes per record (LMDB)
    // - 12 bytes key:
    //      - timestamp : 8 bytes
    //      - chunk index: 4 bytes
    ((page_size - 16) / 2) - 8 - 12
}
