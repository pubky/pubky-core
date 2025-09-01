mod users;
mod signup_codes;
mod sessions;
mod entries;
mod events;

use crate::persistence::{lmdb::{sql_migrator::{entries::migrate_entries, sessions::migrate_sessions, signup_codes::migrate_signup_codes, users::migrate_users}, LmDB}, sql::SqlDb};



/// Migrate the LMDB to the SQL database.
pub async fn migrate_lmdb_to_sql(lmdb: &LmDB, sql_db: &SqlDb) -> anyhow::Result<()> {
    migrate_users(lmdb, sql_db).await?;
    migrate_signup_codes(lmdb, sql_db).await?;
    migrate_sessions(lmdb, sql_db).await?;
    migrate_entries(lmdb, sql_db).await?;
    Ok(())
}