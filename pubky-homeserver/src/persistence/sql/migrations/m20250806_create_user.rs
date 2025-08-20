use async_trait::async_trait;
use pkarr::PublicKey;
use sea_query::{ColumnDef, Expr, Iden, PostgresQueryBuilder, Table};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row, Transaction};

use crate::persistence::sql::{db_connection::SqlDb, migration::MigrationTrait};

const USER_TABLE: &str = "users";

pub struct M20250806CreateUserMigration;

#[async_trait]
impl MigrationTrait for M20250806CreateUserMigration {
    async fn up(
        &self,
        db: &SqlDb,
        tx: &mut Transaction<'static, sqlx::Postgres>,
    ) -> anyhow::Result<()> {
        let statement = Table::create()
            .table(USER_TABLE)
            .if_not_exists()
            .col(
                ColumnDef::new(User::Id)
                    .integer()
                    .primary_key()
                    .auto_increment(),
            )
            .col(
                ColumnDef::new(User::PublicKey)
                    .string_len(52)
                    .not_null()
                    .unique_key(),
            )
            .col(
                ColumnDef::new(User::Disabled)
                    .boolean()
                    .not_null()
                    .default(false),
            )
            .col(
                ColumnDef::new(User::UsedBytes)
                    .big_unsigned()
                    .not_null()
                    .default(0),
            )
            .col(
                ColumnDef::new(User::CreatedAt)
                    .timestamp()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .to_owned();
        let query = statement.build(PostgresQueryBuilder::default());
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        let index = sea_query::Index::create()
            .name("idx_user_public_key")
            .table(USER_TABLE)
            .col(User::PublicKey)
            .index_type(sea_query::IndexType::BTree)
            .to_owned();
        let query = index.build(PostgresQueryBuilder::default());
        sqlx::query(query.as_str()).execute(&mut **tx).await?;
        Ok(())
    }

    fn name(&self) -> &str {
        "m20250806_create_user"
    }
}

#[derive(Iden)]
enum User {
    Id,
    PublicKey,
    CreatedAt,
    Disabled,
    UsedBytes,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct UserEntity {
    pub id: u32,
    pub public_key: PublicKey,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl FromRow<'_, PgRow> for UserEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i32 = row.try_get(User::Id.to_string().as_str())?;
        let raw_pubkey: String = row.try_get(User::PublicKey.to_string().as_str())?;
        let public_key = PublicKey::try_from(raw_pubkey.as_str())
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let disabled: bool = row.try_get(User::Disabled.to_string().as_str())?;
        let raw_used_bytes: i64 = row.try_get(User::UsedBytes.to_string().as_str())?;
        let used_bytes = raw_used_bytes as u64;
        let created_at: sqlx::types::chrono::NaiveDateTime = row.try_get(User::CreatedAt.to_string().as_str())?;
        Ok(UserEntity {
            id: id as u32,
            public_key,
            created_at,
            disabled,
            used_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;
    use sea_query::{Query, SimpleExpr};

    use crate::persistence::sql::migrator::Migrator;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_user_migration() {
        let db = SqlDb::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![Box::new(M20250806CreateUserMigration)])
            .await
            .expect("Should run successfully");

        // Create a user
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USER_TABLE)
            .columns([User::PublicKey])
            .values(vec![SimpleExpr::Value(pubkey.to_string().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder::default());

        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read user
        let statement = Query::select()
            .from(USER_TABLE)
            .columns([User::Id, User::PublicKey, User::CreatedAt, User::Disabled, User::UsedBytes])
            .to_owned();
        let (query, _) = statement.build_sqlx(PostgresQueryBuilder::default());
        let user: UserEntity = sqlx::query_as(query.as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(user.public_key, pubkey);
        assert_eq!(user.disabled, false);
        assert_eq!(user.used_bytes, 0);
    }
}
