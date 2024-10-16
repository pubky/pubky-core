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
        };

        Ok(db)
    }
}
