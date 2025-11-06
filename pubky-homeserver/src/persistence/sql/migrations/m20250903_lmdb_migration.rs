use async_trait::async_trait;
use sea_query::{ColumnDef, Expr, ForeignKey, ForeignKeyAction, Iden, Index, PostgresQueryBuilder, Table};
use sqlx::{postgres::PgRow, FromRow, Row, Transaction};

use crate::persistence::{
    lmdb::{migrate_lmdb_to_sql, tables::users::USERS_TABLE},
    sql::{entities::user::UserIden, migration::MigrationTrait},
};

const TABLE: &str = "entries";

pub struct M20250815CreateEntryMigration;

#[async_trait]
impl MigrationTrait for M20250815CreateEntryMigration {
    async fn up(
        &self,
        tx: &mut Transaction<'static, sqlx::Postgres>,
    ) -> anyhow::Result<()> {
        // Create table
        migrate_lmdb_to_sql(lmdb, tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20250903_lmdb_migration"
    }
}

