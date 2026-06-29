use async_trait::async_trait;
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

/// Adds `allowed_write_paths` TEXT column to both `users` and `signup_codes` tables.
///
/// NULL = unrestricted (all paths allowed), JSON array string = restricted.
pub struct M20260507AddAllowedWritePathsMigration;

#[async_trait]
impl MigrationTrait for M20260507AddAllowedWritePathsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        for table in ["users", "signup_codes"] {
            sqlx::query(&format!(
                "ALTER TABLE {table} ADD COLUMN IF NOT EXISTS allowed_write_paths TEXT"
            ))
            .execute(&mut **tx)
            .await?;
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "m20260507_add_allowed_write_paths"
    }
}
