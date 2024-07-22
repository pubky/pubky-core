use heed::{types::Str, Database, Env, RwTxn};

use super::tables;

pub const TABLES_COUNT: u32 = 2;

pub fn create_users_table(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: tables::users::UsersTable =
        env.create_database(wtxn, Some(tables::users::USERS_TABLE))?;

    Ok(())
}

pub fn create_sessions_table(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<()> {
    let _: tables::sessions::SessionsTable =
        env.create_database(wtxn, Some(tables::sessions::SESSIONS_TABLE))?;

    Ok(())
}
