use async_trait::async_trait;
use sea_query::{
    ColumnDef, Expr, ForeignKey, ForeignKeyAction, Iden, Index, PostgresQueryBuilder, Table,
};
use sqlx::Transaction;

use crate::persistence::sql::{
    entities::user::{UserIden, USER_TABLE},
    migration::MigrationTrait,
};

const TABLE: &str = "path_write_reservations";
const UNIQUE_INDEX: &str = "idx_path_write_reservations_user_path";
const STALE_INDEX: &str = "idx_path_write_reservations_user_created_at";

pub struct M20260612CreatePathWriteReservationsMigration;

#[async_trait]
impl MigrationTrait for M20260612CreatePathWriteReservationsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        let statement = Table::create()
            .table(TABLE)
            .if_not_exists()
            .col(
                ColumnDef::new(PathWriteReservationIden::Id)
                    .big_integer()
                    .primary_key()
                    .auto_increment(),
            )
            .col(
                ColumnDef::new(PathWriteReservationIden::User)
                    .integer()
                    .not_null(),
            )
            .col(
                ColumnDef::new(PathWriteReservationIden::Path)
                    .text()
                    .not_null(),
            )
            .col(
                ColumnDef::new(PathWriteReservationIden::CreatedAt)
                    .timestamp()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .to_owned();
        let query = statement.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        let foreign_key = ForeignKey::create()
            .name("fk_path_write_reservations_user")
            .from(TABLE, PathWriteReservationIden::User)
            .to(USER_TABLE, UserIden::Id)
            .on_delete(ForeignKeyAction::Cascade)
            .to_owned();
        let query = foreign_key.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        let unique_index = Index::create()
            .name(UNIQUE_INDEX)
            .table(TABLE)
            .col(PathWriteReservationIden::User)
            .col(PathWriteReservationIden::Path)
            .unique()
            .index_type(sea_query::IndexType::BTree)
            .to_owned();
        let query = unique_index.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        let stale_index = Index::create()
            .name(STALE_INDEX)
            .table(TABLE)
            .col(PathWriteReservationIden::User)
            .col(PathWriteReservationIden::CreatedAt)
            .index_type(sea_query::IndexType::BTree)
            .to_owned();
        let query = stale_index.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260612_create_path_write_reservations"
    }
}

#[derive(Iden)]
enum PathWriteReservationIden {
    Id,
    User,
    Path,
    CreatedAt,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::{
        migrations::M20250806CreateUserMigration, migrator::Migrator, SqlDb,
    };

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_write_reservations_migration() {
        let db = SqlDb::test_without_migrations().await;
        Migrator::new(&db)
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20260612CreatePathWriteReservationsMigration),
            ])
            .await
            .unwrap();

        let table_exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.tables
                WHERE table_name = 'path_write_reservations'
            )
            "#,
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(table_exists);

        let unique_index_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE tablename = $1 AND indexname = $2)",
        )
        .bind(TABLE)
        .bind(UNIQUE_INDEX)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(unique_index_exists);

        let stale_index_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE tablename = $1 AND indexname = $2)",
        )
        .bind(TABLE)
        .bind(STALE_INDEX)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(stale_index_exists);

        let foreign_key_exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM information_schema.table_constraints
                WHERE table_name = $1
                  AND constraint_name = 'fk_path_write_reservations_user'
                  AND constraint_type = 'FOREIGN KEY'
            )
            "#,
        )
        .bind(TABLE)
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert!(foreign_key_exists);
    }
}
