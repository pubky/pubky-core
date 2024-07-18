use heed::{types::Str, Database, Env, RwTxn};

use super::tables;

pub const TABLES_COUNT: u32 = 1;

pub fn create_users_table(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: tables::users::UsersTable = env.create_database(wtxn, None)?;

    Ok(())
}
