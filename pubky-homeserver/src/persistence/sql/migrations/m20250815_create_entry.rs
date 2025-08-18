use async_trait::async_trait;
use sea_query::{ColumnDef, Expr, ForeignKey, ForeignKeyAction, Iden, Table};
use sqlx::{postgres::PgRow, FromRow, Row, Transaction};

use crate::persistence::{
    lmdb::tables::users::USERS_TABLE,
    sql::{db_connection::DbConnection, entities::user::UserIden, migration::MigrationTrait},
};

const TABLE: &str = "entries";

pub struct M20250815CreateEntryMigration;

#[async_trait]
impl MigrationTrait for M20250815CreateEntryMigration {
    async fn up(
        &self,
        db: &DbConnection,
        tx: &mut Transaction<'static, sqlx::Postgres>,
    ) -> anyhow::Result<()> {
        // Create table
        let statement = Table::create()
            .table(TABLE)
            .if_not_exists()
            .col(
                ColumnDef::new(EntryIden::Id)
                    .big_integer()
                    .primary_key()
                    .auto_increment(),
            )
            .col(
                ColumnDef::new(EntryIden::Path)
                    .string()
                    .not_null(),
            )
            .col(
                ColumnDef::new(EntryIden::User)
                    .integer()
                    .not_null(),
            )
            .col(
                ColumnDef::new(EntryIden::ContentHash)
                    .blob()
                    .not_null(),
            )
            .col(
                ColumnDef::new(EntryIden::ContentLength)
                    .big_unsigned()
                    .not_null(),
            )
            .col(
                ColumnDef::new(EntryIden::ContentType)
                    .string()
                    .not_null(),
            )
            .col(
                ColumnDef::new(EntryIden::CreatedAt)
                    .timestamp()
                    .not_null()
                    .default(Expr::current_timestamp()),
            )
            .to_owned();
        let query = db.build_schema(statement);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        // Create foreign key
        let foreign_key = ForeignKey::create()
            .name("fk_entry_user")
            .from(TABLE, EntryIden::User)
            .to(USERS_TABLE, UserIden::Id)
            .on_delete(ForeignKeyAction::Cascade)
            .to_owned();
        let query = db.build_schema(foreign_key);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20250815_create_entry"
    }
}

#[derive(Iden)]
enum EntryIden {
    Id,
    Path,
    User,
    ContentHash,
    ContentLength,
    ContentType,
    CreatedAt,
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct EntryEntity {
    pub id: i64,
    pub user_id: i32,
    pub path: String,
    pub content_hash: Vec<u8>,
    pub content_length: i64,
    pub content_type: String,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for EntryEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EntryIden::Id.to_string().as_str())?;
        let user_id: i32 = row.try_get(EntryIden::User.to_string().as_str())?;
        let path: String = row.try_get(EntryIden::Path.to_string().as_str())?;
        let content_hash: Vec<u8> = row.try_get(EntryIden::ContentHash.to_string().as_str())?;
        let content_length: i64 = row.try_get(EntryIden::ContentLength.to_string().as_str())?;
        let content_type: String = row.try_get(EntryIden::ContentType.to_string().as_str())?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EntryIden::CreatedAt.to_string().as_str())?;
        Ok(EntryEntity {
            id,
            user_id,
            path,
            content_hash,
            content_length,
            content_type,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;
    use sea_query::{Query, SimpleExpr};

    use crate::persistence::{
        lmdb::tables::users::USERS_TABLE,
        sql::{
            entities::user::UserIden, migrations::M20250806CreateUserMigration, migrator::Migrator,
        },
    };

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_event_migration() {
        let db = DbConnection::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250815CreateEntryMigration),
            ])
            .await
            .expect("Should run successfully");

        // Create a user
        let pubkey = Keypair::random().public_key();
        let statement = Query::insert()
            .into_table(USERS_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![SimpleExpr::Value(pubkey.to_string().into())])
            .unwrap()
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        let bytes: Vec<u8> = vec![0; 32];
        // let bytes = 
        // Create an event
        let statement = Query::insert()
            .into_table(TABLE)
            .columns([
                EntryIden::User,
                EntryIden::Path,
                EntryIden::ContentHash,
                EntryIden::ContentLength,
                EntryIden::ContentType,
            ])
            .values(vec![
                SimpleExpr::Value(1.into()),
                SimpleExpr::Value("/test".into()),
                SimpleExpr::Value(bytes.clone().into()),
                SimpleExpr::Value(100.into()),
                SimpleExpr::Value("text/plain".into()),
            ])
            .unwrap()
            .to_owned();
        let (query, values) = db.build_query(statement);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read event
        let statement = Query::select()
            .from(TABLE)
            .columns([
                EntryIden::Id,
                EntryIden::User,
                EntryIden::Path,
                EntryIden::ContentHash,
                EntryIden::ContentLength,
                EntryIden::ContentType,
                EntryIden::CreatedAt,
            ])
            .to_owned();
        let (query, _) = db.build_query(statement);
        let entry: EntryEntity = sqlx::query_as(query.as_str())
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(entry.user_id, 1);
        assert_eq!(entry.path, "/test");
        assert_eq!(entry.content_hash, vec![0; 32]);
        assert_eq!(entry.content_length, 100);
        assert_eq!(entry.content_type, "text/plain");
    }
}
