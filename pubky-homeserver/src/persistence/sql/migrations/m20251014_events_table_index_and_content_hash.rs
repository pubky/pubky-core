use async_trait::async_trait;
use sea_query::{ColumnDef, Index, PostgresQueryBuilder, Table};
use sqlx::Transaction;

use crate::persistence::sql::{entities::event::EventIden, migration::MigrationTrait};

const TABLE: &str = "events";
const INDEX_NAME: &str = "idx_events_user_path_id";

pub struct M20251014EventsTableIndexAndContentHashMigration;

#[async_trait]
impl MigrationTrait for M20251014EventsTableIndexAndContentHashMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        // Create index on (user, path, id) for efficient per-user event streaming with path filtering
        let statement = Index::create()
            .name(INDEX_NAME)
            .table(TABLE)
            .col("user")
            .col("path")
            .col("id")
            .to_owned();
        let query = statement.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        // Add nullable content_hash column for tracking file content hashes
        let statement = Table::alter()
            .table(TABLE)
            .add_column(ColumnDef::new(EventIden::ContentHash).binary().null())
            .to_owned();
        let query = statement.build(PostgresQueryBuilder);
        sqlx::query(query.as_str()).execute(&mut **tx).await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20251014_enhance_events_table"
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
    async fn test_enhance_events_table_migration() {
        let db = SqlDb::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250814CreateEventMigration),
                Box::new(M20251014EventsTableIndexAndContentHashMigration),
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

        // Verify index exists
        let index_check = sqlx::query(
            "SELECT indexname FROM pg_indexes WHERE tablename = 'events' AND indexname = $1",
        )
        .bind(INDEX_NAME)
        .fetch_optional(db.pool())
        .await
        .unwrap();

        assert!(index_check.is_some(), "Index {} should exist", INDEX_NAME);

        // Verify index columns are (user, path, id)
        let index_columns: Vec<(i16, String)> = sqlx::query_as(
            "SELECT a.attnum, a.attname
             FROM pg_index i
             JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = ANY(i.indkey)
             WHERE i.indrelid = 'events'::regclass
             AND i.indexrelid = $1::regclass
             ORDER BY array_position(i.indkey, a.attnum)",
        )
        .bind(INDEX_NAME)
        .fetch_all(db.pool())
        .await
        .unwrap();

        assert_eq!(index_columns.len(), 3, "Index should have 3 columns");
        assert_eq!(index_columns[0].1, "user", "First column should be 'user'");
        assert_eq!(index_columns[1].1, "path", "Second column should be 'path'");
        assert_eq!(index_columns[2].1, "id", "Third column should be 'id'");
    }
}
