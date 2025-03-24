use heed::Env;
use super::tables::Tables;

mod m0;

/// Run the migrations.
pub fn run(env: &Env) -> anyhow::Result<Tables> {
    let mut wtxn = env.write_txn()?;

    m0::run(env, &mut wtxn)?;

    let tables = Tables::new(env, &mut wtxn)?;

    wtxn.commit()?;

    Ok(tables)
}
