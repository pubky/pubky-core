use async_trait::async_trait;
use sqlx::Transaction;

use crate::data_directory::user_limit_config::UserLimitConfig;
use crate::persistence::sql::migration::MigrationTrait;

/// Adds per-user limit columns to the users table and backfills existing users
/// with the deploy-time defaults so every user row has explicit limits.
///
/// **Important:** The backfill "freezes" the current deploy-time defaults onto
/// every existing user row. After this migration has run, changing the TOML
/// config values will only affect *newly created* users. Existing users retain
/// the limits that were written during this one-time backfill. To change an
/// existing user's limits after the fact, use the admin API
/// (`PUT /users/{pubkey}/limits`) or `DELETE` their custom limits to revert
/// them to the (possibly updated) deploy-time defaults.
///
/// The deploy-time defaults are read from `[general]` in `config.toml` via
/// [`UserLimitConfig::from_general_toml`]. The fields used are:
/// - `storage_limit_mb` (or deprecated `user_storage_quota_mb`)
/// - `max_sessions`
/// - `user_rate_read`
/// - `user_rate_write`
pub struct M20260327AddUserLimitColumnsMigration {
    pub defaults: UserLimitConfig,
}

#[async_trait]
impl MigrationTrait for M20260327AddUserLimitColumnsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        // 1. Add the columns.
        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS limit_storage_quota_mb BIGINT")
            .execute(&mut **tx)
            .await?;
        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS limit_max_sessions INTEGER")
            .execute(&mut **tx)
            .await?;
        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS limit_rate_read VARCHAR(32)")
            .execute(&mut **tx)
            .await?;
        sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS limit_rate_write VARCHAR(32)")
            .execute(&mut **tx)
            .await?;

        // 2. Backfill existing users with the configured defaults.
        //    This migration runs exactly once (tracked by name in the migrations table).
        //    Only touches rows where all limit columns are still NULL (i.e. never set).
        //
        //    NOTE: This writes the current deploy-time defaults into each user row,
        //    effectively "freezing" them. Future TOML config changes will NOT
        //    retroactively update these users — use the admin API for that.
        let defaults = &self.defaults;
        sqlx::query(
            "UPDATE users
             SET limit_storage_quota_mb = $1,
                 limit_max_sessions = $2,
                 limit_rate_read = $3,
                 limit_rate_write = $4
             WHERE limit_storage_quota_mb IS NULL
               AND limit_max_sessions IS NULL
               AND limit_rate_read IS NULL
               AND limit_rate_write IS NULL",
        )
        .bind(defaults.storage_quota_mb.map(|v| v as i64))
        .bind(defaults.max_sessions.map(|v| v as i32))
        .bind(defaults.rate_read_str())
        .bind(defaults.rate_write_str())
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260327_add_user_limit_columns"
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::data_directory::user_limit_config::UserLimitConfig;
    use crate::persistence::sql::migrations::{
        M20250806CreateUserMigration, M20250812CreateSignupCodeMigration,
        M20250813CreateSessionMigration, M20250814CreateEventMigration,
        M20250815CreateEntryMigration, M20251014EventsTableIndexAndContentHashMigration,
    };
    use crate::persistence::sql::migrator::Migrator;
    use crate::persistence::sql::sql_db::SqlDb;
    use pubky_common::crypto::Keypair;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
    use sea_query_binder::SqlxBinder;

    use crate::persistence::sql::entities::user::{UserIden, USER_TABLE};

    /// Helper: run all prior migrations plus the user limit columns migration.
    async fn run_with_defaults(db: &SqlDb, defaults: UserLimitConfig) {
        let migrator = Migrator::new(db, defaults.clone());
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250812CreateSignupCodeMigration),
                Box::new(M20250813CreateSessionMigration),
                Box::new(M20250814CreateEventMigration),
                Box::new(M20250815CreateEntryMigration),
                Box::new(M20251014EventsTableIndexAndContentHashMigration),
                Box::new(M20260327AddUserLimitColumnsMigration { defaults }),
            ])
            .await
            .expect("migrations should succeed");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_adds_columns_successfully() {
        let db = SqlDb::test_without_migrations().await;
        run_with_defaults(&db, UserLimitConfig::default()).await;

        // Verify columns exist by inserting and reading a user with limit fields
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USER_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![SimpleExpr::Value(pubkey.z32().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read back — limit columns should be NULL
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT limit_storage_quota_mb, limit_max_sessions, limit_rate_read, limit_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_applies_non_null_defaults() {
        let db = SqlDb::test_without_migrations().await;

        // Run prior migrations first (without the limit columns migration)
        let migrator = Migrator::new(&db, UserLimitConfig::default());
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250812CreateSignupCodeMigration),
                Box::new(M20250813CreateSessionMigration),
                Box::new(M20250814CreateEventMigration),
                Box::new(M20250815CreateEntryMigration),
                Box::new(M20251014EventsTableIndexAndContentHashMigration),
            ])
            .await
            .unwrap();

        // Insert a user before running the limit columns migration
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USER_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![SimpleExpr::Value(pubkey.z32().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Now run the limit columns migration with non-null defaults
        let defaults = UserLimitConfig {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(crate::data_directory::quota_config::BandwidthBudget::from_str("100mb/m").unwrap()),
            rate_write: None,
        };
        migrator
            .run_migrations(vec![Box::new(M20260327AddUserLimitColumnsMigration {
                defaults,
            })])
            .await
            .unwrap();

        // Verify the existing user was backfilled
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT limit_storage_quota_mb, limit_max_sessions, limit_rate_read, limit_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.0, Some(500));
        assert_eq!(row.1, Some(10));
        assert_eq!(row.2, Some("100mb/m".to_string()));
        assert_eq!(row.3, None); // rate_write was None in defaults
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_with_unlimited_defaults_leaves_nulls() {
        let db = SqlDb::test_without_migrations().await;

        // Run prior migrations
        let migrator = Migrator::new(&db, UserLimitConfig::default());
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250812CreateSignupCodeMigration),
                Box::new(M20250813CreateSessionMigration),
                Box::new(M20250814CreateEventMigration),
                Box::new(M20250815CreateEntryMigration),
                Box::new(M20251014EventsTableIndexAndContentHashMigration),
            ])
            .await
            .unwrap();

        // Insert a user before running the limit columns migration
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USER_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![SimpleExpr::Value(pubkey.z32().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Run with all-None defaults — backfill sets NULLs (same as initial state)
        migrator
            .run_migrations(vec![Box::new(M20260327AddUserLimitColumnsMigration {
                defaults: UserLimitConfig::default(),
            })])
            .await
            .unwrap();

        // Columns remain NULL (unlimited defaults = NULL values)
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT limit_storage_quota_mb, limit_max_sessions, limit_rate_read, limit_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }
}
