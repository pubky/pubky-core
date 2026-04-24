use async_trait::async_trait;
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

pub struct M20260327AddSignupCodeLimitColumnsMigration;

#[async_trait]
impl MigrationTrait for M20260327AddSignupCodeLimitColumnsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_storage_quota_mb BIGINT",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_max_sessions INTEGER",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_rate_read VARCHAR(32)",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_rate_write VARCHAR(32)",
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260327_add_signup_code_limit_columns"
    }
}
