use pkarr::PublicKey;
use sea_query::{ColumnDef, Expr, Iden, Table};
use sqlx::{any::{AnyRow, AnyValueRef}, sqlite::SqliteRow, FromRow, Row, Transaction};
use async_trait::async_trait;

use crate::persistence::sql::{db_connection::DbConnection, migration::MigrationTrait};

pub struct CreateUserMigration;

#[async_trait]
impl MigrationTrait for CreateUserMigration {
    async fn up(&self, db: &DbConnection, tx: &mut Transaction<'static, sqlx::Any>) -> anyhow::Result<()> {
        let query = Table::create().table(User::Table)
            .if_not_exists()
            .col(ColumnDef::new(User::Id).string_len(52).not_null().primary_key())
            .col(ColumnDef::new(User::Disabled).integer().not_null().default(0))
            .col(ColumnDef::new(User::UsedBytes).big_unsigned().not_null().default(0))
            .col(ColumnDef::new(User::CreatedAt).timestamp().not_null().default(Expr::current_timestamp()))
            .to_owned().build_any(db.schema_builder());
        sqlx::query(query.as_str()).execute(&mut **tx).await?;
        Ok(())
    }

    fn name(&self) -> &str {
        "m20250806_create_user"
    }
}

#[derive(Iden)]
enum User {
    Table,
    Id,
    CreatedAt,
    Disabled,
    UsedBytes,
}


#[derive(Debug, PartialEq, Eq, Clone)]
struct UserEntity {
    pub id: PublicKey,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl FromRow<'_, AnyRow> for UserEntity {
    fn from_row(row: &AnyRow) -> Result<Self, sqlx::Error> {
        let id_name = User::Id.to_string();
        let raw_pubkey: String = row.try_get(id_name.as_str())?;
        let id = PublicKey::try_from(raw_pubkey.as_str()).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let raw_disabled: i64 = row.try_get(User::Disabled.to_string().as_str())?;
        let disabled = raw_disabled != 0;
        let raw_used_bytes: i64 = row.try_get(User::UsedBytes.to_string().as_str())?;
        let used_bytes = raw_used_bytes as u64;
        let raw_created_at: String = row.try_get(User::CreatedAt.to_string().as_str())?;
        let created_at = sqlx::types::chrono::NaiveDateTime::parse_from_str(&raw_created_at, "%Y-%m-%d %H:%M:%S%.f").map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        Ok(UserEntity {
            id,
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
    use sea_query_binder::SqlxBinder;

    use crate::persistence::sql::migrator::Migrator;

    use super::*;

    #[tokio::test]
    async fn test_create_user_migration() {
        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator.run_migrations(vec![Box::new(CreateUserMigration)]).await.expect("Should run successfully");

        // Create a user
        let pubkey = Keypair::random().public_key();
        let (query, values) = Query::insert().into_table(User::Table)
            .columns([User::Id])
            .values(vec![
                SimpleExpr::Value(pubkey.to_string().into()),
            ]).unwrap()
            .build_any_sqlx(db.schema_builder());

        sqlx::query_with(query.as_str(), values).execute(db.pool()).await.unwrap();


        // Read user
        let (query, _) = Query::select().from(User::Table)
            .columns([User::Id, User::CreatedAt, User::Disabled, User::UsedBytes])
            .build_any_sqlx(db.query_builder());
        let user: UserEntity = sqlx::query_as(query.as_str()).fetch_one(db.pool()).await.unwrap();
        assert_eq!(user.id, pubkey);
        assert_eq!(user.disabled, false);
        assert_eq!(user.used_bytes, 0);
        println!("{:?}", user);
    }
}