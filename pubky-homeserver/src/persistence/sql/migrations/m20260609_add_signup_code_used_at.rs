use async_trait::async_trait;
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

/// Adds the timestamp for signup token consumption.
pub struct M20260609AddSignupCodeUsedAtMigration;

#[async_trait]
impl MigrationTrait for M20260609AddSignupCodeUsedAtMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        sqlx::query("ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS used_at TIMESTAMP")
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    fn name(&self) -> &str {
        "m20260609_add_signup_code_used_at"
    }
}
