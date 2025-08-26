mod users;
mod signup_codes;
mod sessions;

use crate::persistence::{lmdb::{sql_migrator::{signup_codes::migrate_signup_codes, users::migrate_users}, LmDB}, sql::SqlDb};



/// Migrate the LMDB to the SQL database.
pub async fn migrate_lmdb_to_sql(lmdb: &LmDB, sql_db: &SqlDb) -> anyhow::Result<()> {
    migrate_users(lmdb, sql_db).await?;
    migrate_signup_codes(lmdb, sql_db).await?;
    
    Ok(())
}