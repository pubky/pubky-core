//! Migration from LMDB to SQL database.
//! TODO: Remove this module after the migration is complete.

mod entries;
mod events;
mod sessions;
mod signup_codes;
mod users;

use crate::persistence::{
    lmdb::{
        sql_migrator::{
            entries::migrate_entries, events::migrate_events, sessions::migrate_sessions,
            signup_codes::migrate_signup_codes, users::migrate_users,
        },
        LmDB,
    },
    sql::{Migrator, SqlDb},
};

const MIGRATION_NAME: &str = "m20250915_lmdb_to_sql_migration";

/// Migrate the LMDB to the SQL database.
/// Does everything in one transaction. If one migration fails, the entire transaction is rolled back.
pub async fn migrate_lmdb_to_sql(lmdb: LmDB, sql_db: &SqlDb) -> anyhow::Result<()> {
    let mut tx = sql_db.pool().begin().await?;
    migrate_users(lmdb.clone(), &mut (&mut tx).into()).await?;
    migrate_signup_codes(lmdb.clone(), &mut (&mut tx).into()).await?;
    migrate_sessions(lmdb.clone(), &mut (&mut tx).into()).await?;
    migrate_entries(lmdb.clone(), &mut (&mut tx).into()).await?;
    migrate_events(lmdb.clone(), &mut (&mut tx).into()).await?;

    // Mark the migration as done.
    let migrator = Migrator::new(sql_db);
    migrator
        .mark_migration_as_done(&mut tx, MIGRATION_NAME)
        .await?;

    tx.commit().await?;
    Ok(())
}

/// Check if the migration is needed.
pub async fn is_migration_needed(sql_db: &SqlDb) -> anyhow::Result<bool> {
    let migrator = Migrator::new(sql_db);
    let already_applied = migrator
        .has_migration_already_been_applied(MIGRATION_NAME)
        .await?;
    Ok(!already_applied)
}
