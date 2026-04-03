use async_trait::async_trait;
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

/// Adds per-user limit columns to both the `users` and `signup_codes` tables,
/// then backfills only `quota_storage_mb` on existing rows.
///
/// The other three fields (`max_sessions`, `rate_read`, `rate_write`) are new
/// concepts and start as NULL (= `Default`, meaning use system default).
///
/// **Users backfill:** Sets `quota_storage_mb` from the deploy-time config on
/// every existing user whose column is NULL. After this migration, config
/// changes only affect newly created users.
///
/// **Signup codes backfill:** Sets `quota_storage_mb` on unused tokens only.
///
/// `default_storage_quota_mb`:
/// - `None` → config says unlimited → NULL (no limit)
/// - `Some(n)` → store n as the value
pub struct M20260327AddResourceQuotaColumnsMigration {
    /// The storage default from `[general].user_storage_quota_mb`.
    /// `None` means unlimited (0 in config → unlimited → NULL in DB).
    pub default_storage_quota_mb: Option<i64>,
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

/// Backfill only `quota_storage_mb` using the deploy-time default.
async fn backfill_storage_quota(
    tx: &mut Transaction<'static, sqlx::Postgres>,
    sql: &str,
    storage_val: Option<i64>,
) -> anyhow::Result<()> {
    sqlx::query(sql)
        .bind(storage_val)
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

        // 2. Backfill existing users with only storage_quota_mb.
        // Other columns stay NULL (= no limit).
        // None → unlimited → NULL (no backfill needed), Some(n) → store n.
        let storage_val = self.default_storage_quota_mb;
        backfill_storage_quota(
            tx,
            "UPDATE users
             SET quota_storage_mb = $1
             WHERE quota_storage_mb IS NULL",
            storage_val,
        )
        .await?;

        // 3. Backfill unused tokens with storage_quota_mb only.
        backfill_storage_quota(
            tx,
            "UPDATE signup_codes
             SET quota_storage_mb = $1
             WHERE used_by IS NULL
               AND quota_storage_mb IS NULL",
            storage_val,
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
    use super::*;
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
    async fn run_migrations(db: &SqlDb, storage_default: Option<Option<i64>>) {
        let migrator = Migrator::new(db);
        let mut migrations: Vec<Box<dyn crate::persistence::sql::migration::MigrationTrait>> = vec![
            Box::new(M20250806CreateUserMigration),
            Box::new(M20250812CreateSignupCodeMigration),
            Box::new(M20250813CreateSessionMigration),
            Box::new(M20250814CreateEventMigration),
            Box::new(M20250815CreateEntryMigration),
            Box::new(M20251014EventsTableIndexAndContentHashMigration),
        ];
        if let Some(default_storage) = storage_default {
            migrations.push(Box::new(M20260327AddResourceQuotaColumnsMigration {
                default_storage_quota_mb: default_storage,
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
        run_migrations(&db, Some(None)).await;

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
        run_migrations(&db, Some(None)).await;

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
    async fn test_backfill_users_with_storage_default() {
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

        // Run migration with storage default = 500 MB
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                default_storage_quota_mb: Some(500),
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
        // Only storage_quota_mb should be backfilled; others stay NULL
        assert_eq!(row.0, Some(500));
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
        assert_eq!(row.3, None);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_users_unlimited_storage() {
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

        // Run migration with storage default = None (unlimited → NULL)
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                default_storage_quota_mb: None,
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
        // Unlimited → NULL in DB (no limit)
        assert_eq!(row.0, None);
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
        assert_eq!(row.3, None);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_backfill_unused_signup_tokens() {
        let db = SqlDb::test_without_migrations().await;
        run_migrations(&db, None).await;

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

        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20260327AddResourceQuotaColumnsMigration {
                default_storage_quota_mb: Some(500),
            })])
            .await
            .unwrap();

        // Unused token should have storage backfilled
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT quota_storage_mb, quota_max_sessions, quota_rate_read, quota_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(unused_code.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.0, Some(500));
        assert_eq!(row.1, None);
        assert_eq!(row.2, None);
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
