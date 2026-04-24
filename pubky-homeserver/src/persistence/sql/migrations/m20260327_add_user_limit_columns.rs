use async_trait::async_trait;
use sqlx::Transaction;

use crate::data_directory::user_limit_config::UserLimitConfig;
use crate::persistence::sql::migration::MigrationTrait;

/// Adds per-user limit columns to the users table and backfills existing users
/// with the deploy-time defaults so every user row has explicit limits.
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
        //    Only touches rows where all limit columns are still NULL (i.e. never set).
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
        .bind(defaults.rate_read.as_deref())
        .bind(defaults.rate_write.as_deref())
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260327_add_user_limit_columns"
    }
}
