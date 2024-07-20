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

        let db = DB { env };

        db.run_migrations();

        Ok(db)
    }

    fn run_migrations(&self) -> anyhow::Result<()> {
        let mut wtxn = self.env.write_txn()?;

        migrations::create_users_table(&self.env, &mut wtxn);
        migrations::create_sessions_table(&self.env, &mut wtxn);

        wtxn.commit()?;

        Ok(())
    }
}
