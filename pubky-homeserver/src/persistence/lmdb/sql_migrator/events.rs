use pubky_common::timestamp::Timestamp;
use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;

use crate::{
    persistence::{
        lmdb::{tables::events::Event, LmDB},
        sql::{
            event::{EventIden, EventType, EVENT_TABLE},
            user::UserRepository,
            UnifiedExecutor,
        },
    },
    shared::{timestamp_to_sqlx_datetime, webdav::EntryPath},
};

fn pubky_url_into_entry_path(url: &str) -> anyhow::Result<EntryPath> {
    let path = url
        .split("://")
        .nth(1)
        .ok_or(anyhow::anyhow!("Invalid URL"))?;
    let entry_path = path.parse::<EntryPath>()?;
    Ok(entry_path)
}

/// Create a new signup code.
/// The executor can either be db.pool() or a transaction.
pub async fn create<'a>(
    timestamp: &Timestamp,
    event: &Event,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    let created_at = timestamp_to_sqlx_datetime(timestamp);
    let event_type = match event {
        Event::Put(_) => EventType::Put,
        Event::Delete(_) => EventType::Delete,
    };
    let entry_path = pubky_url_into_entry_path(event.url())?;
    let user_id = UserRepository::get_id(entry_path.pubkey(), executor).await?;
    let statement = Query::insert()
        .into_table(EVENT_TABLE)
        .columns([
            EventIden::Type,
            EventIden::User,
            EventIden::Path,
            EventIden::CreatedAt,
        ])
        .values(vec![
            SimpleExpr::Value(event_type.to_string().into()),
            SimpleExpr::Value(user_id.into()),
            SimpleExpr::Value(entry_path.path().as_str().into()),
            SimpleExpr::Value(created_at.into()),
        ])
        .expect("Failed to build insert statement")
        .to_owned();

    let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

    let con = executor.get_con().await?;
    sqlx::query_with(&query, values).execute(con).await?;
    Ok(())
}

pub async fn migrate_events<'a>(
    lmdb: LmDB,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    tracing::info!("Migrating events from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut count = 0;
    for record in lmdb.tables.events.iter(&lmdb_txn)? {
        let (timestamp, bytes) = record?;
        let timestamp: Timestamp = timestamp.to_string().try_into()?;
        let event = Event::deserialize(bytes)?;
        create(&timestamp, &event, executor).await?;
        count += 1;
    }
    tracing::info!("Migrated {} events", count);
    Ok(())
}

#[cfg(test)]
mod tests {

    use pkarr::Keypair;
    use pubky_common::timestamp::Timestamp;
    use sqlx::types::chrono::DateTime;

    use crate::{
        persistence::sql::{event::EventRepository, SqlDb},
        shared::webdav::WebDavPath,
    };

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();

        let user1_pubkey = Keypair::random().public_key();
        UserRepository::create(&user1_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();

        // PUT pubky://user1_pubkey/folder1/file1.txt
        let entry_path1 = EntryPath::new(
            user1_pubkey.clone(),
            WebDavPath::new("/folder1/file1.txt").unwrap(),
        );
        let event1 = Event::Put(format!("pubky://{}", entry_path1.as_str()));
        let timestamp1 = Timestamp::now();
        lmdb.tables
            .events
            .put(
                &mut wtxn,
                timestamp1.to_string().as_str(),
                &event1.serialize(),
            )
            .unwrap();

        // DELETE pubky://user1_pubkey/folder1/file1.txt
        let event2 = Event::Delete(format!("pubky://{}", entry_path1.as_str()));
        let timestamp2 = Timestamp::now();
        lmdb.tables
            .events
            .put(
                &mut wtxn,
                timestamp2.to_string().as_str(),
                &event2.serialize(),
            )
            .unwrap();

        let user2_pubkey = Keypair::random().public_key();
        UserRepository::create(&user2_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        let entry_path2 = EntryPath::new(
            user2_pubkey.clone(),
            WebDavPath::new("/folder2/file1.txt").unwrap(),
        );
        // PUT pubky://user2_pubkey/folder2/file1.txt
        let event3 = Event::Put(format!("pubky://{}", entry_path2.as_str()));
        let timestamp3 = Timestamp::now();
        lmdb.tables
            .events
            .put(
                &mut wtxn,
                timestamp3.to_string().as_str(),
                &event3.serialize(),
            )
            .unwrap();

        wtxn.commit().unwrap();

        // Migrate
        migrate_events(lmdb.clone(), &mut sql_db.pool().into())
            .await
            .unwrap();

        // Check
        let events: Vec<crate::persistence::sql::event::EventEntity> =
            EventRepository::get_by_cursor(None, None, Some(10), &mut sql_db.pool().into())
                .await
                .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, EventType::Put);
        assert_eq!(events[0].path, entry_path1);
        assert_eq!(events[0].user_pubkey, user1_pubkey);
        assert_eq!(
            events[0].created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            DateTime::from_timestamp((timestamp1.as_u64() / 1_000_000) as i64, 0)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(events[1].event_type, EventType::Delete);
        assert_eq!(events[1].path, entry_path1);
        assert_eq!(events[1].user_pubkey, user1_pubkey);
        assert_eq!(
            events[1].created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            DateTime::from_timestamp((timestamp2.as_u64() / 1_000_000) as i64, 0)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(events[2].event_type, EventType::Put);
        assert_eq!(events[2].path, entry_path2);
        assert_eq!(events[2].user_pubkey, user2_pubkey);
        assert_eq!(
            events[2].created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            DateTime::from_timestamp((timestamp3.as_u64() / 1_000_000) as i64, 0)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
    }
}
