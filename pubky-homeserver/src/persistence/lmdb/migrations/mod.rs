use heed::Env;

mod m0;
mod m220420251247_add_user_disabled_used_bytes;

/// Run the migrations.
pub fn run(env: &Env) -> anyhow::Result<()> {
    let mut wtxn = env.write_txn()?;

    m0::run(env, &mut wtxn)?;
    m220420251247_add_user_disabled_used_bytes::run(env, &mut wtxn)?;

    wtxn.commit()?;

    Ok(())
}
