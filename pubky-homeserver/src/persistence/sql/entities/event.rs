use std::{fmt::Display, str::FromStr};

use pkarr::PublicKey;
use pubky_common::timestamp::Timestamp;
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{
    postgres::PgRow,
    types::chrono::{DateTime, Utc},
    FromRow, Row,
};

use crate::{
    constants::{DEFAULT_LIST_LIMIT, DEFAULT_MAX_LIST_LIMIT},
    persistence::sql::{
        entities::user::{UserIden, USER_TABLE},
        UnifiedExecutor,
    },
    shared::{
        timestamp_to_sqlx_datetime,
        webdav::{EntryPath, WebDavPath},
    },
};

pub const EVENT_TABLE: &str = "events";

/// Repository that handles all the queries regarding the EventEntity.
pub struct EventRepository;

impl EventRepository {
    /// Create a new event.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(
        user_id: i32,
        event_type: EventType,
        path: &WebDavPath,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<i64, sqlx::Error> {
        Self::create_with_timestamp(user_id, event_type, path, &Utc::now(), executor).await
    }

    /// Create a new event with a specific timestamp.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create_with_timestamp<'a>(
        user_id: i32,
        event_type: EventType,
        path: &WebDavPath,
        created_at: &DateTime<Utc>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<i64, sqlx::Error> {
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
                SimpleExpr::Value(path.as_str().into()),
                SimpleExpr::Value(created_at.naive_utc().into()),
            ])
            .expect("Failed to build insert statement")
            .returning_col(EventIden::Id)
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

        let con = executor.get_con().await?;
        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let event_id: i64 = ret_row.try_get(EventIden::Id.to_string().as_str())?;
        Ok(event_id)
    }

    /// Parse the cursor to the event id.
    /// The cursor can be either a new cursor format or a legacy cursor format.
    /// The new cursor format is a u64.
    /// The legacy cursor format is a timestamp.
    /// The cursor is the id of the last event in the list.
    /// If you don't to use the cursor, set it to "0".
    pub async fn parse_cursor<'a>(
        cursor: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<i64, sqlx::Error> {
        if let Ok(cursor) = cursor.parse::<u64>() {
            // Is new cursor format
            return Ok(cursor as i64);
        }
        // Check for the legacy cursor format
        let timestamp: Timestamp = match cursor.to_string().try_into() {
            Ok(timestamp) => timestamp,
            Err(e) => return Err(sqlx::Error::Decode(e.into())),
        };

        // Check the timestamp with the database to convert it to the event id
        let datetime = timestamp_to_sqlx_datetime(&timestamp);
        let statement = Query::select()
            .column((EVENT_TABLE, EventIden::Id))
            .from(EVENT_TABLE)
            .and_where(Expr::col((EVENT_TABLE, EventIden::CreatedAt)).eq(datetime))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let event_id: i64 = ret_row.try_get(EventIden::Id.to_string().as_str())?;
        Ok(event_id)
    }

    /// Get a list of events by the cursor. The cursor is the id of the last event in the list.
    /// If you don't to use the cursor, set it to 0.
    /// The limit is the maximum number of events to return.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_cursor<'a>(
        cursor: Option<i64>,
        limit: Option<u16>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        let cursor = cursor.unwrap_or(0);
        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let limit = limit.min(DEFAULT_MAX_LIST_LIMIT);

        let statement = Query::select()
            .columns([
                (EVENT_TABLE, EventIden::Id),
                (EVENT_TABLE, EventIden::User),
                (EVENT_TABLE, EventIden::Type),
                (EVENT_TABLE, EventIden::User),
                (EVENT_TABLE, EventIden::Path),
                (EVENT_TABLE, EventIden::CreatedAt),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .from(EVENT_TABLE)
            .left_join(
                USER_TABLE,
                Expr::col((EVENT_TABLE, EventIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).gt(cursor))
            .limit(limit as u64)
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let events: Vec<EventEntity> = sqlx::query_as_with(&query, values).fetch_all(con).await?;
        Ok(events)
    }
}

#[derive(Iden)]
pub enum EventIden {
    Id,
    Type,
    User,
    Path,
    CreatedAt,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum EventType {
    Put,
    Delete,
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            EventType::Put => "PUT",
            EventType::Delete => "DEL",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PUT" => Ok(EventType::Put),
            "DEL" => Ok(EventType::Delete),
            _ => Err(format!("Failed to parse invalid event type: {}", s)),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EventEntity {
    pub id: i64,
    pub user_id: i32,
    pub user_pubkey: PublicKey,
    pub event_type: EventType,
    pub path: EntryPath,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for EventEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EventIden::Id.to_string().as_str())?;
        let user_id: i32 = row.try_get(EventIden::User.to_string().as_str())?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_pubkey =
            PublicKey::from_str(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let event_type: String = row.try_get(EventIden::Type.to_string().as_str())?;
        let event_type =
            EventType::from_str(&event_type).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let user_public_key: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_public_key =
            PublicKey::from_str(&user_public_key).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let path: String = row.try_get(EventIden::Path.to_string().as_str())?;
        let path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EventIden::CreatedAt.to_string().as_str())?;
        Ok(EventEntity {
            id,
            event_type,
            user_id,
            user_pubkey,
            path: EntryPath::new(user_public_key, path),
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use super::*;
    use crate::persistence::sql::{entities::user::UserRepository, SqlDb};
    use std::ops::Add;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_list_event() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Test create session
        for _ in 0..10 {
            let _ = EventRepository::create(
                user.id,
                EventType::Put,
                &WebDavPath::new("/test").unwrap(),
                &mut db.pool().into(),
            )
            .await
            .unwrap();
        }

        // Test get session
        let events = EventRepository::get_by_cursor(Some(5), Some(4), &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].id, 6);
        assert_eq!(events[0].user_id, user.id);
        assert_eq!(
            events[0].path,
            EntryPath::new(user_pubkey, WebDavPath::new("/test").unwrap())
        );
        assert_eq!(events[0].event_type, EventType::Put);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_transform_legacy_cursor() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let mut timestamp_events = Vec::new();
        // Test create session
        for i in 0..10 {
            let timestamp = Timestamp::now().add(1_000_000 * i); // Add 1s for each event
            let created_at = timestamp_to_sqlx_datetime(&timestamp);
            let event_id = EventRepository::create_with_timestamp(
                user.id,
                EventType::Put,
                &WebDavPath::new("/test").unwrap(),
                &created_at,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
            timestamp_events.push((timestamp, event_id));
        }

        // Test get session
        for (timestamp, should_be_event_id) in timestamp_events {
            let event_id =
                EventRepository::parse_cursor(&timestamp.to_string(), &mut db.pool().into())
                    .await
                    .unwrap();
            assert_eq!(should_be_event_id, event_id);
        }
    }
}
