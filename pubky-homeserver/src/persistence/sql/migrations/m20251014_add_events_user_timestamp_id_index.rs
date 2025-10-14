use async_trait::async_trait;
use sea_query::{Index, PostgresQueryBuilder};
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

const TABLE: &str = "events";
const INDEX_NAME: &str = "idx_events_user_timestamp_id";

pub struct M20251014AddEventsUserTimestampIdIndexMigration;

#[async_trait]
impl MigrationTrait for M20251014AddEventsUserTimestampIdIndexMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        // Create index on (user, created_at, id)
        let statement = Index::create()
            .name(INDEX_NAME)
            .table(TABLE)
            .col("user")
            .col("created_at")
            .col("id")
            .to_owned();
        let query = statement.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20251014_add_events_user_timestamp_id_index"
    }
}
