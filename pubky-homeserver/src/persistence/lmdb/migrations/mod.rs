use heed::Env;

mod m0;
mod m202506021102_entry_location;
mod m220420251247_add_user_disabled_used_bytes;
mod m290520251418_migrate_content_types;

/// Run the migrations.
pub fn run(env: &Env) -> anyhow::Result<()> {
    let mut wtxn = env.write_txn()?;

    m0::run(env, &mut wtxn)?;
    m220420251247_add_user_disabled_used_bytes::run(env, &mut wtxn)?;
    m202506021102_entry_location::run(env, &mut wtxn)?;
    m290520251418_migrate_content_types::run(env, &mut wtxn)?;
    wtxn.commit()?;

    Ok(())
}
