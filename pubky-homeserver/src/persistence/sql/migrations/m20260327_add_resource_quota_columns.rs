use async_trait::async_trait;
use sqlx::Transaction;

use crate::data_directory::user_resource_quota::UserResourceQuota;
use crate::persistence::sql::migration::MigrationTrait;

/// Adds per-user limit columns to both the `users` and `signup_codes` tables,
/// then backfills existing rows with the deploy-time defaults.
///
/// **Users backfill:** Freezes the current deploy-time defaults onto every
/// existing user row whose limit columns are all NULL. After this migration,
/// changing the TOML config only affects *newly created* users. Use the admin
/// API (`PUT /users/{pubkey}/resource-quotas`) to change existing users.
///
/// **Signup codes backfill:** Writes deploy-time defaults onto every *unused*
/// token whose limit columns are all NULL. At signup time the token's limits
/// are copied directly to the new user row — there is no fallback to
/// deploy-time defaults — so pre-existing unused tokens must carry explicit
/// values.
///
/// The deploy-time defaults are read from `[general]` in `config.toml` via
/// [`UserResourceQuota::from_general_toml`].
pub struct M20260327AddResourceQuotaColumnsMigration {
    pub defaults: UserResourceQuota,
}

/// Add the four limit columns to the given table.
async fn add_resource_quota_columns(
    tx: &mut Transaction<'static, sqlx::Postgres>,
    table: &str,
) -> anyhow::Result<()> {
    for (col, typ) in [
        ("quota_storage_mb", "BIGINT"),
        ("quota_max_sessions", "INTEGER"),
        ("quota_rate_read", "VARCHAR(32)"),
        ("quota_rate_write", "VARCHAR(32)"),
    ] {
        sqlx::query(&format!(
            "ALTER TABLE {table} ADD COLUMN IF NOT EXISTS {col} {typ}"
        ))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Bind the four limit values and execute an UPDATE statement.
async fn backfill_resource_quotas(
    tx: &mut Transaction<'static, sqlx::Postgres>,
    sql: &str,
    defaults: &UserResourceQuota,
) -> anyhow::Result<()> {
    sqlx::query(sql)
        .bind(defaults.storage_quota_mb_i64())
        .bind(defaults.max_sessions_i32())
        .bind(defaults.rate_read_str())
        .bind(defaults.rate_write_str())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[async_trait]
impl MigrationTrait for M20260327AddResourceQuotaColumnsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        // 1. Add limit columns to both tables.
        add_resource_quota_columns(tx, "users").await?;
        add_resource_quota_columns(tx, "signup_codes").await?;

        // 2. Backfill existing users with the configured defaults.
        backfill_resource_quotas(
            tx,
            "UPDATE users
             SET quota_storage_mb = $1, quota_max_sessions = $2,
                 quota_rate_read = $3, quota_rate_write = $4
             WHERE quota_storage_mb IS NULL
               AND quota_max_sessions IS NULL
               AND quota_rate_read IS NULL
               AND quota_rate_write IS NULL",
            &self.defaults,
        )
        .await?;

        // 3. Backfill unused tokens with deploy-time defaults.
        backfill_resource_quotas(
            tx,
            "UPDATE signup_codes
             SET quota_storage_mb = $1, quota_max_sessions = $2,
                 quota_rate_read = $3, quota_rate_write = $4
             WHERE used_by IS NULL
               AND quota_storage_mb IS NULL
               AND quota_max_sessions IS NULL
               AND quota_rate_read IS NULL
               AND quota_rate_write IS NULL",
            &self.defaults,
        )
        .await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260327_add_resource_quota_columns"
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::data_directory::user_resource_quota::UserResourceQuota;
    use crate::persistence::sql::entities::signup_code::{
        SignupCodeId, SignupCodeIden, SIGNUP_CODE_TABLE,
    };
    use crate::persistence::sql::entities::user::{UserIden, USER_TABLE};
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

    /// Helper: run all prior migrations, optionally including the limit columns migration.
    async fn run_migrations(db: &SqlDb, limit_defaults: Option<UserResourceQuota>) {
        let migrator = Migrator::new(db);
        let mut migrations: Vec<Box<dyn crate::persistence::sql::migration::MigrationTrait>> = vec![
            Box::new(M20250806CreateUserMigration),
            Box::new(M20250812CreateSignupCodeMigration),
            Box::new(M20250813CreateSessionMigration),
            Box::new(M20250814CreateEventMigration),
            Box::new(M20250815CreateEntryMigration),
            Box::new(M20251014EventsTableIndexAndContentHashMigration),
        ];
        if let Some(defaults) = limit_defaults {
            migrations.push(Box::new(M20260327AddResourceQuotaColumnsMigration {
                defaults,
            }));
        }
        migrator
            .run_migrations(migrations)
            .await
            .expect("migrations should succeed");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_adds_columns_to_users() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, Some(UserResourceQuota::default())).await;

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

        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_adds_columns_to_signup_codes() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, Some(UserResourceQuota::default())).await;

        let code_id = SignupCodeId::random();
        let statement = Query::insert()
            .into_table(SIGNUP_CODE_TABLE)
            .columns([SignupCodeIden::Id])
            .values(vec![SimpleExpr::Value(code_id.to_string().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(code_id.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_users_with_defaults() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, None).await;

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

        let defaults = UserResourceQuota {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(
                crate::data_directory::quota_config::BandwidthBudget::from_str("100mb/m").unwrap(),
            ),
            rate_write: None,
        };
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                defaults,
            })])
            .await
            .unwrap();

        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.0, Some(500));
        assert_eq!(row.1, Some(10));
        assert_eq!(row.2, Some("100mb/m".to_string()));
        assert_eq!(row.3, None);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_users_with_unlimited_defaults_leaves_nulls() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, None).await;

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

        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                defaults: UserResourceQuota::default(),
            })])
            .await
            .unwrap();

        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM users WHERE public_key = $1",
        )
        .bind(pubkey.z32())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_unused_signup_tokens_with_defaults() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, None).await;

        // Insert an unused token and a used token before the limit migration
        let unused_code = SignupCodeId::random();
        let used_code = SignupCodeId::random();
        sqlx::query("INSERT INTO signup_codes (id) VALUES ($1)")
            .bind(unused_code.to_string())
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO signup_codes (id, used_by) VALUES ($1, $2)")
            .bind(used_code.to_string())
            .bind("some_pubkey_z32")
            .execute(db.pool())
            .await
            .unwrap();

        let defaults = UserResourceQuota {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some(
                crate::data_directory::quota_config::BandwidthBudget::from_str("100mb/m").unwrap(),
            ),
            rate_write: None,
        };
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                defaults,
            })])
            .await
            .unwrap();

        // Unused token should be backfilled
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(unused_code.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.0, Some(500));
        assert_eq!(row.1, Some(10));
        assert_eq!(row.2, Some("100mb/m".to_string()));
        assert_eq!(row.3, None);

        // Used token should NOT be backfilled
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(used_code.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));
    }
}
