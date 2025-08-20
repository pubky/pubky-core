use crate::persistence::sql::db_connection::SqlDb;
use async_trait::async_trait;
use sqlx::Transaction;

#[async_trait]
pub trait MigrationTrait: Send + Sync {
    /// Run the migration.
    /// Use the tx to perform all the necessary operations.
    /// In case of an error, the tx is automatically rolled back.
    async fn up(&self, db: &SqlDb, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()>;

    /// The name of the migration.
    /// This is used to identify the migration in the database.
    /// It should be unique and consistent.
    fn name(&self) -> &str;
}
