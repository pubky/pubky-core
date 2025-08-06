use sea_query::{ColumnDef, Table};
use sqlx::Transaction;
use async_trait::async_trait;

use crate::persistence::sql::{db_connection::DbConnection, migrations::migration::MigrationTrait};

pub struct CreateUserMigration;

#[async_trait]
impl MigrationTrait for CreateUserMigration {
    async fn up(&self, _db: &DbConnection, tx: &mut Transaction<'static, sqlx::Any>) -> anyhow::Result<()> {
        // let query = Table::create().table("users")
        //     .if_not_exists()
        //     .col(ColumnDef::new("id").string_len(52).not_null().primary_key())
        //     .col(ColumnDef::new(Name).string().not_null())
        //     .to_owned();
        // tx.execute(query).await?;
        // Ok(())
    }

    fn name(&self) -> &str {
        "create_user"
    }
}

