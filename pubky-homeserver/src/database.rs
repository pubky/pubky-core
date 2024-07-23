use std::fs;
use std::path::Path;

use heed::{types::Str, Database, Env, EnvOpenOptions, RwTxn};

mod migrations;
pub mod tables;

use migrations::TABLES_COUNT;

#[derive(Debug, Clone)]
pub struct DB {
    pub(crate) env: Env,
}

impl DB {
    pub fn open(storage: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(storage).unwrap();

        let env = unsafe { EnvOpenOptions::new().max_dbs(TABLES_COUNT).open(storage) }?;

        migrations::run(&env);

        let db = DB { env };

        Ok(db)
    }
}
