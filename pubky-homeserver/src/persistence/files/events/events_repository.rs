use std::{fmt::Display, str::FromStr};

use pubky_common::crypto::Hash;
use pubky_common::timestamp::Timestamp;
use sea_query::{Expr, Iden, LikeExpr, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{
    postgres::PgRow,
    types::chrono::{DateTime, Utc},
    Row,
};

use crate::{
    constants::{DEFAULT_LIST_LIMIT, DEFAULT_MAX_LIST_LIMIT},
    persistence::{
        files::events::EventEntity,
        sql::{
            user::{UserIden, USER_TABLE},
            UnifiedExecutor,
        },
    },
    shared::{timestamp_to_sqlx_datetime, webdav::EntryPath},
};

pub const EVENT_TABLE: &str = "events";

/// Cursor for pagination in event queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct EventCursor(i64);

impl EventCursor {
    /// Create a new cursor from an event ID
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    /// Get the underlying ID value
    pub fn id(&self) -> i64 {
        self.0
    }
}

impl Display for EventCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for EventCursor {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(EventCursor(s.parse()?))
    }
}

/// Repository that handles all the queries regarding the EventEntity.
pub struct EventRepository;

impl EventRepository {
    /// Create a new event.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(
        user_id: i32,
        event_type: EventType,
        path: &EntryPath,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventEntity, sqlx::Error> {
        Self::create_with_timestamp(user_id, event_type, path, &Utc::now(), executor).await
    }

    /// Create a new event with a specific timestamp.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create_with_timestamp<'a>(
        user_id: i32,
        event_type: EventType,
        path: &EntryPath,
        created_at: &DateTime<Utc>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventEntity, sqlx::Error> {
        let mut columns = vec![
            EventIden::Type,
            EventIden::User,
            EventIden::Path,
            EventIden::CreatedAt,
        ];
        let mut values = vec![
            SimpleExpr::Value(event_type.to_string().into()),
            SimpleExpr::Value(user_id.into()),
            SimpleExpr::Value(path.path().as_str().into()),
            SimpleExpr::Value(created_at.naive_utc().into()),
        ];

        if let Some(hash) = event_type.content_hash() {
            columns.push(EventIden::ContentHash);
            values.push(SimpleExpr::Value(hash.as_bytes().to_vec().into()));
        }

        let statement = Query::insert()
            .into_table(EVENT_TABLE)
            .columns(columns)
            .values(values)
            .expect("Failed to build insert statement")
            .returning_col(EventIden::Id)
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

        let con = executor.get_con().await?;
        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let event_id: i64 = ret_row.try_get(EventIden::Id.to_string().as_str())?;
        Ok(EventEntity {
            id: event_id,
            user_id,
            user_pubkey: path.pubkey().clone(),
            event_type,
            path: path.clone(),
            created_at: created_at.naive_utc(),
        })
    }

    /// Parse the cursor to a Cursor.
    /// The cursor can be either a new cursor format or a legacy cursor format.
    /// The new cursor format is the ID of the last event - a u64.
    /// The legacy cursor format is a timestamp.
    /// If you don't want to use the cursor, set it to "0".
    pub async fn parse_cursor<'a>(
        cursor: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventCursor, sqlx::Error> {
        if let Ok(cursor) = cursor.parse::<EventCursor>() {
            // Is new cursor format
            return Ok(cursor);
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
        Ok(EventCursor::new(event_id))
    }

    /// Get a list of events with per-user cursors.
    /// The limit is the maximum total number of events to return across all users.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_user_cursors<'a>(
        user_cursors: Vec<(i32, Option<EventCursor>)>,
        reverse: bool,
        path_prefix: Option<&str>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        if user_cursors.is_empty() {
            return Ok(Vec::new());
        }

        // Build a UNION query for each user with their individual cursor
        // This ensures we get events after each user's last seen position
        let order = if reverse {
            sea_query::Order::Desc
        } else {
            sea_query::Order::Asc
        };

        let mut union_queries = Vec::new();

        for (user_id, cursor) in user_cursors {
            let mut statement = Query::select()
                .columns([
                    (EVENT_TABLE, EventIden::Id),
                    (EVENT_TABLE, EventIden::User),
                    (EVENT_TABLE, EventIden::Type),
                    (EVENT_TABLE, EventIden::Path),
                    (EVENT_TABLE, EventIden::CreatedAt),
                    (EVENT_TABLE, EventIden::ContentHash),
                ])
                .column((USER_TABLE, UserIden::PublicKey))
                .from(EVENT_TABLE)
                .left_join(
                    USER_TABLE,
                    Expr::col((EVENT_TABLE, EventIden::User))
                        .eq(Expr::col((USER_TABLE, UserIden::Id))),
                )
                .and_where(Expr::col((EVENT_TABLE, EventIden::User)).eq(user_id))
                .to_owned();

            // Note: paths in the database are stored without the user pubkey prefix (e.g., "/pub/files/doc.txt")
            if let Some(prefix) = path_prefix {
                // Escape special LIKE characters: %, _, and \
                let escaped_prefix = prefix
                    .replace('\\', "\\\\")
                    .replace('_', "\\_")
                    .replace('%', "\\%");
                let like_pattern = format!("{}%", escaped_prefix);
                statement = statement
                    .and_where(
                        Expr::col((EVENT_TABLE, EventIden::Path))
                            .like(LikeExpr::new(like_pattern).escape('\\')),
                    )
                    .to_owned();
            }

            if let Some(cursor) = cursor {
                if reverse {
                    statement = statement
                        .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).lt(cursor.id()))
                        .to_owned();
                } else {
                    statement = statement
                        .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).gt(cursor.id()))
                        .to_owned();
                }
            }

            union_queries.push(statement);
        }

        // Combine all user queries with UNION ALL and wrap in subquery
        let mut combined_query = union_queries[0].clone();
        for query in union_queries.iter().skip(1) {
            combined_query = combined_query
                .union(sea_query::UnionType::All, query.clone())
                .to_owned();
        }

        // Wrap the UNION in a subquery and apply ordering and limit
        // This is necessary because we can't order UNION results directly
        let subquery_alias = sea_query::Alias::new("union_result");
        combined_query = Query::select()
            .from_subquery(combined_query, subquery_alias.clone())
            .column((subquery_alias.clone(), EventIden::Id))
            .column((subquery_alias.clone(), EventIden::User))
            .column((subquery_alias.clone(), EventIden::Type))
            .column((subquery_alias.clone(), EventIden::Path))
            .column((subquery_alias.clone(), EventIden::CreatedAt))
            .column((subquery_alias.clone(), EventIden::ContentHash))
            .column((subquery_alias.clone(), UserIden::PublicKey))
            .order_by((subquery_alias, EventIden::Id), order)
            .limit(DEFAULT_LIST_LIMIT as u64)
            .to_owned();

        let (query, values) = combined_query.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let events: Vec<EventEntity> = sqlx::query_as_with(&query, values).fetch_all(con).await?;
        Ok(events)
    }

    /// Get a list of events by the cursor.
    /// The limit is the maximum number of events to return.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_cursor<'a>(
        cursor: Option<EventCursor>,
        limit: Option<u16>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        let cursor = cursor.unwrap_or(EventCursor::new(0));
        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let limit = limit.min(DEFAULT_MAX_LIST_LIMIT);

        let statement = Query::select()
            .columns([
                (EVENT_TABLE, EventIden::Id),
                (EVENT_TABLE, EventIden::User),
                (EVENT_TABLE, EventIden::Type),
                (EVENT_TABLE, EventIden::Path),
                (EVENT_TABLE, EventIden::CreatedAt),
                (EVENT_TABLE, EventIden::ContentHash),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .from(EVENT_TABLE)
            .left_join(
                USER_TABLE,
                Expr::col((EVENT_TABLE, EventIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).gt(cursor.id()))
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
    ContentHash,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum EventType {
    Put { content_hash: Hash },
    Delete,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::Put { .. } => "PUT",
            EventType::Delete => "DEL",
        }
    }

    pub fn content_hash(&self) -> Option<&Hash> {
        match self {
            EventType::Put { content_hash } => Some(content_hash),
            EventType::Delete => None,
        }
    }
}

impl Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

    use crate::{
        persistence::sql::{user::UserRepository, SqlDb},
        shared::webdav::WebDavPath,
    };

    use super::*;
    use std::ops::Add;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_list_event() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Test create session
        for _ in 0..10 {
            let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test").unwrap());
            let _ = EventRepository::create(
                user.id,
                EventType::Put {
                    content_hash: Hash::from_bytes([0; 32]),
                },
                &path,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
        }

        // Test get session
        let events = EventRepository::get_by_cursor(
            Some(EventCursor::new(5)),
            Some(4),
            &mut db.pool().into(),
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].id, 6);
        assert_eq!(events[0].user_id, user.id);
        assert_eq!(
            events[0].path,
            EntryPath::new(user_pubkey, WebDavPath::new("/test").unwrap())
        );
        assert!(matches!(events[0].event_type, EventType::Put { .. }));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
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
            let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test").unwrap());
            let event = EventRepository::create_with_timestamp(
                user.id,
                EventType::Put {
                    content_hash: Hash::from_bytes([0; 32]),
                },
                &path,
                &created_at,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
            timestamp_events.push((timestamp, event.id));
        }

        // Test get session
        for (timestamp, should_be_event_id) in timestamp_events {
            let cursor =
                EventRepository::parse_cursor(&timestamp.to_string(), &mut db.pool().into())
                    .await
                    .unwrap();
            assert_eq!(should_be_event_id, cursor.id());
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_parse_cursor_backwards_compatibility() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Create test events with specific timestamps
        let mut events = Vec::new();
        for i in 0..5 {
            let timestamp = Timestamp::now().add(1_000_000 * i); // Add 1s for each event
            let created_at = timestamp_to_sqlx_datetime(&timestamp);
            let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test").unwrap());
            let event = EventRepository::create_with_timestamp(
                user.id,
                EventType::Put {
                    content_hash: Hash::from_bytes([0; 32]),
                },
                &path,
                &created_at,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
            events.push((event, timestamp));
        }

        let test_event = &events[2].0; // Use the third event for testing
        let test_timestamp = &events[2].1;

        // Test 1: New format - just the id as string
        let new_format_cursor = test_event.id.to_string();
        let parsed_new = EventRepository::parse_cursor(&new_format_cursor, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(parsed_new, test_event.cursor());

        // Test 2: Legacy format - timestamp only
        let legacy_timestamp_format = test_timestamp.to_string();
        let parsed_timestamp =
            EventRepository::parse_cursor(&legacy_timestamp_format, &mut db.pool().into())
                .await
                .unwrap();
        assert_eq!(parsed_timestamp, test_event.cursor());

        // Test 3: Use parsed cursors in get_by_cursor to verify they work correctly
        for (cursor_str, test_name) in [
            (new_format_cursor, "new format"),
            (legacy_timestamp_format, "legacy timestamp"),
        ] {
            let cursor_id = EventRepository::parse_cursor(&cursor_str, &mut db.pool().into())
                .await
                .unwrap();

            let events_after =
                EventRepository::get_by_cursor(Some(cursor_id), None, &mut db.pool().into())
                    .await
                    .unwrap();

            // Should get events after the cursor (events[3] and events[4])
            assert_eq!(
                events_after.len(),
                2,
                "Failed for cursor format: {}",
                test_name
            );
            assert_eq!(
                events_after[0].id, events[3].0.id,
                "Failed for cursor format: {}",
                test_name
            );
            assert_eq!(
                events_after[1].id, events[4].0.id,
                "Failed for cursor format: {}",
                test_name
            );
        }
    }
}
