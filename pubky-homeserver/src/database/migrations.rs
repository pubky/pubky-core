use heed::{types::Str, Database, Env, RwTxn};

mod m0;

use super::tables;

pub const TABLES_COUNT: u32 = 4;

pub fn run(env: &Env) -> anyhow::Result<()> {
    let mut wtxn = env.write_txn()?;

    m0::run(env, &mut wtxn);

    wtxn.commit()?;

    Ok(())
}
