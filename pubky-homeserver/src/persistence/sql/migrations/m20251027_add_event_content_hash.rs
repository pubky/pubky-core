use async_trait::async_trait;
use sea_query::{ColumnDef, PostgresQueryBuilder, Table};
use sqlx::Transaction;

use crate::persistence::sql::{entities::event::EventIden, migration::MigrationTrait};

const TABLE: &str = "events";

pub struct M20251027AddEventContentHashMigration;

#[async_trait]
impl MigrationTrait for M20251027AddEventContentHashMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        // Add nullable content_hash column
        let statement = Table::alter()
            .table(TABLE)
            .add_column(ColumnDef::new(EventIden::ContentHash).binary().null())
            .to_owned();
        let query = statement.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20251027_add_event_content_hash"
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;
    use pubky_common::crypto::Hash;
    use sea_query::{Iden, Query, SimpleExpr};
    use sea_query_binder::SqlxBinder;

    use crate::persistence::{
        lmdb::tables::users::USERS_TABLE,
        sql::{
            entities::{event::EventIden, user::UserIden},
            migrations::{M20250806CreateUserMigration, M20250814CreateEventMigration},
            migrator::Migrator,
            SqlDb,
        },
    };
    use sqlx::{postgres::PgRow, FromRow, Row};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Clone)]
    struct EventEntity {
        pub id: i64,
        pub event_type: String,
        pub user_id: i32,
        pub path: String,
        pub created_at: sqlx::types::chrono::NaiveDateTime,
        pub content_hash: Option<Vec<u8>>,
    }

    impl FromRow<'_, PgRow> for EventEntity {
        fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
            let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
            let event_type: String = row.try_get(EventIden::Type.to_string().as_str())?;
            let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
            let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
            let created_at: sqlx::types::chrono::NaiveDateTime =
                row.try_get(EventIden::CreatedAt.to_string().as_str())?;
            let content_hash: Option<Vec<u8>> =
                row.try_get(EventIden::ContentHash.to_string().as_str())?;
            Ok(EventEntity {
                id,
                event_type,
                user_id,
                path,
                created_at,
                content_hash,
            })
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_add_event_content_hash_migration() {
        let db = SqlDb::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250814CreateEventMigration),
                Box::new(M20251027AddEventContentHashMigration),
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
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Create an event without content_hash
        let statement = Query::insert()
            .into_table(TABLE)
            .columns([EventIden::Type, EventIden::User, EventIden::Path])
            .values(vec![
                SimpleExpr::Value("put".into()),
                SimpleExpr::Value(1.into()),
                SimpleExpr::Value("/test".into()),
            ])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Create an event with content_hash
        let hash = Hash::from_bytes([42u8; 32]);
        let statement = Query::insert()
            .into_table(TABLE)
            .columns([
                EventIden::Type,
                EventIden::User,
                EventIden::Path,
                EventIden::ContentHash,
            ])
            .values(vec![
                SimpleExpr::Value("put".into()),
                SimpleExpr::Value(1.into()),
                SimpleExpr::Value("/test2".into()),
                SimpleExpr::Value(hash.as_bytes().to_vec().into()),
            ])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Read events
        let statement = Query::select()
            .from(TABLE)
            .columns([
                EventIden::Id,
                EventIden::Type,
                EventIden::User,
                EventIden::Path,
                EventIden::CreatedAt,
                EventIden::ContentHash,
            ])
            .order_by(EventIden::Id, sea_query::Order::Asc)
            .to_owned();
        let (query, _) = statement.build_sqlx(PostgresQueryBuilder);
        let events: Vec<EventEntity> = sqlx::query_as(query.as_str())
            .fetch_all(db.pool())
            .await
            .unwrap();

        assert_eq!(events.len(), 2);

        // First event has no content_hash
        assert_eq!(events[0].event_type, "put");
        assert_eq!(events[0].user_id, 1);
        assert_eq!(events[0].path, "/test");
        assert_eq!(events[0].content_hash, None);

        // Second event has content_hash
        assert_eq!(events[1].event_type, "put");
        assert_eq!(events[1].user_id, 1);
        assert_eq!(events[1].path, "/test2");
        assert_eq!(events[1].content_hash, Some(hash.as_bytes().to_vec()));
    }
}
