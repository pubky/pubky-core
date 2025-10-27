use std::{fmt::Display, str::FromStr};

use pkarr::PublicKey;
use pubky_common::crypto::Hash;
use pubky_common::timestamp::Timestamp;
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{
    postgres::PgRow,
    types::chrono::{DateTime, NaiveDateTime, Utc},
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

/// Response structure for event streams.
/// This represents the data format returned by the `/events-stream` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventResponse {
    /// The type of event (PUT or DEL)
    pub event_type: EventType,
    /// The full pubky path (e.g., "pubky://user_pubkey/pub/example.txt")
    pub path: String,
    /// Cursor for pagination (timestamp:id format)
    pub cursor: String,
    /// Optional content hash (blake3) in hex format
    pub content_hash: Option<String>,
}

impl EventResponse {
    /// Create an EventResponse from an EventEntity
    pub fn from_entity(entity: &EventEntity) -> Self {
        let cursor = EventCursor::new(entity.created_at, entity.id);
        Self {
            event_type: entity.event_type.clone(),
            path: format!("pubky://{}", entity.path.as_str()),
            cursor: cursor.to_string(),
            content_hash: entity.content_hash.map(|h| h.to_hex().to_string()),
        }
    }

    /// Format as SSE event data.
    /// Returns the multiline data field content.
    /// Each line will be prefixed with "data: " by the SSE library.
    /// Format:
    /// ```text
    /// data: pubky://user_pubkey/pub/example.txt
    /// data: cursor: 00331BD814YCT:42
    /// data: content_hash: abc123... (optional, only if present)
    /// ```
    pub fn to_sse_data(&self) -> String {
        let mut lines = vec![self.path.clone(), format!("cursor: {}", self.cursor)];
        if let Some(hash) = &self.content_hash {
            lines.push(format!("content_hash: {}", hash));
        }
        lines.join("\n")
    }
}

/// Cursor for pagination in event queries.
/// Format: "timestamp:id" where timestamp is a NaiveDateTime and id is the event ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventCursor {
    pub timestamp: NaiveDateTime,
    pub id: i64,
}

impl EventCursor {
    pub fn new(timestamp: NaiveDateTime, id: i64) -> Self {
        Self { timestamp, id }
    }
}

impl Display for EventCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Format: timestamp:id (e.g., "00331BD814YCT:42")
        // Convert NaiveDateTime to Timestamp string format
        let micros = self.timestamp.and_utc().timestamp_micros() as u64;
        let timestamp: Timestamp = micros.into();
        write!(f, "{}:{}", timestamp, self.id)
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
        content_hash: Option<Hash>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventEntity, sqlx::Error> {
        Self::create_with_timestamp(
            user_id,
            event_type,
            path,
            content_hash,
            &Utc::now(),
            executor,
        )
        .await
    }

    /// Create a new event with a specific timestamp.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create_with_timestamp<'a>(
        user_id: i32,
        event_type: EventType,
        path: &EntryPath,
        content_hash: Option<Hash>,
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

        if let Some(hash) = content_hash {
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
            content_hash,
        })
    }

    /// Parse the cursor to an EventCursor.
    /// The cursor can be in multiple formats for backwards compatibility:
    /// 1. New format: "timestamp:id" (e.g., "00331BD814YCT:42")
    /// 2. Legacy format (timestamp only): A timestamp that needs to be looked up in the database
    ///
    /// If you don't want to use a cursor, set it to "0".
    pub async fn parse_cursor<'a>(
        cursor: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventCursor, sqlx::Error> {
        // Try parsing as new format: "timestamp:id"
        if let Some((timestamp_str, id_str)) = cursor.split_once(':') {
            // Try to parse the timestamp part as a Timestamp
            if let Ok(timestamp) = timestamp_str.to_string().try_into() {
                if let Ok(id) = id_str.parse::<i64>() {
                    let timestamp: Timestamp = timestamp;
                    let datetime = timestamp_to_sqlx_datetime(&timestamp);
                    return Ok(EventCursor::new(datetime.naive_utc(), id));
                }
            }
        }

        // Try parsing as legacy format (timestamp only)
        let timestamp: Timestamp = match cursor.to_string().try_into() {
            Ok(timestamp) => timestamp,
            Err(e) => return Err(sqlx::Error::Decode(e.into())),
        };

        // Check the timestamp with the database to convert it to the event id
        let datetime = timestamp_to_sqlx_datetime(&timestamp);
        let statement = Query::select()
            .columns([
                (EVENT_TABLE, EventIden::Id),
                (EVENT_TABLE, EventIden::CreatedAt),
            ])
            .from(EVENT_TABLE)
            .and_where(Expr::col((EVENT_TABLE, EventIden::CreatedAt)).eq(datetime.naive_utc()))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let event_id: i64 = ret_row.try_get(EventIden::Id.to_string().as_str())?;
        let created_at: NaiveDateTime =
            ret_row.try_get(EventIden::CreatedAt.to_string().as_str())?;
        Ok(EventCursor::new(created_at, event_id))
    }

    /// Get a list of events by the cursor.
    /// The limit is the maximum number of events to return.
    /// The executor can either be db.pool() or a transaction.
    /// This uses the (user, created_at, id) index for efficient querying when user_id is provided.
    pub async fn get_by_cursor<'a>(
        user_ids: Option<Vec<i32>>,
        cursor: Option<EventCursor>,
        limit: Option<u16>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        let limit = limit.unwrap_or(DEFAULT_LIST_LIMIT);
        let limit = limit.min(DEFAULT_MAX_LIST_LIMIT);

        let mut statement = Query::select()
            .columns([
                (EVENT_TABLE, EventIden::Id),
                (EVENT_TABLE, EventIden::User),
                (EVENT_TABLE, EventIden::Type),
                (EVENT_TABLE, EventIden::User),
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
            .to_owned();

        // Filter by users if provided (uses idx_events_user_timestamp_id index)
        if let Some(uids) = user_ids {
            statement = statement
                .and_where(Expr::col((EVENT_TABLE, EventIden::User)).is_in(uids))
                .to_owned();
        }

        // Add cursor condition to use the index: (created_at, id) > (cursor.timestamp, cursor.id)
        if let Some(cursor) = cursor {
            statement = statement
                .and_where(
                    Expr::col((EVENT_TABLE, EventIden::CreatedAt))
                        .gt(cursor.timestamp)
                        .or(Expr::col((EVENT_TABLE, EventIden::CreatedAt))
                            .eq(cursor.timestamp)
                            .and(Expr::col((EVENT_TABLE, EventIden::Id)).gt(cursor.id))),
                )
                .to_owned();
        }

        statement = statement
            .order_by((EVENT_TABLE, EventIden::CreatedAt), sea_query::Order::Asc)
            .order_by((EVENT_TABLE, EventIden::Id), sea_query::Order::Asc)
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
    pub content_hash: Option<Hash>,
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

        // Read optional content_hash
        let content_hash: Option<Vec<u8>> =
            row.try_get(EventIden::ContentHash.to_string().as_str())?;
        let content_hash = content_hash.and_then(|bytes| {
            let hash_bytes: [u8; 32] = bytes.try_into().ok()?;
            Some(Hash::from_bytes(hash_bytes))
        });

        Ok(EventEntity {
            id,
            event_type,
            user_id,
            user_pubkey,
            path: EntryPath::new(user_public_key, path),
            created_at,
            content_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use super::*;
    use crate::persistence::sql::{entities::user::UserRepository, SqlDb};
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

        // Test create session - First get the 4th event to establish our cursor
        for _ in 0..10 {
            let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test").unwrap());
            let _ = EventRepository::create(
                user.id,
                EventType::Put,
                &path,
                None,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
        }

        // Get first 4 events to establish cursor from the 4th event
        let first_4_events =
            EventRepository::get_by_cursor(None, None, Some(4), &mut db.pool().into())
                .await
                .unwrap();
        assert_eq!(first_4_events.len(), 4);
        let cursor_event = &first_4_events[3]; // 4th event (0-indexed)

        // Test get session - Get event with id=5 using cursor from 4th event
        let event5_cursor = EventRepository::get_by_cursor(
            None,
            Some(EventCursor::new(cursor_event.created_at, cursor_event.id)),
            Some(1),
            &mut db.pool().into(),
        )
        .await
        .unwrap();
        assert_eq!(event5_cursor[0].id, cursor_event.id + 1);

        let cursor = EventCursor::new(event5_cursor[0].created_at, event5_cursor[0].id);

        let events =
            EventRepository::get_by_cursor(None, Some(cursor), Some(4), &mut db.pool().into())
                .await
                .unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].id, cursor.id + 1);
        assert_eq!(events[0].user_id, user.id);
        assert_eq!(
            events[0].path,
            EntryPath::new(user_pubkey, WebDavPath::new("/test").unwrap())
        );
        assert_eq!(events[0].event_type, EventType::Put);
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
                EventType::Put,
                &path,
                None,
                &created_at,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
            timestamp_events.push((timestamp, event.id, event.created_at));
        }

        // Test legacy timestamp format parsing
        for (timestamp, should_be_event_id, should_be_timestamp) in timestamp_events {
            let cursor =
                EventRepository::parse_cursor(&timestamp.to_string(), &mut db.pool().into())
                    .await
                    .unwrap();
            assert_eq!(should_be_event_id, cursor.id);
            assert_eq!(should_be_timestamp, cursor.timestamp);
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
                EventType::Put,
                &path,
                None,
                &created_at,
                &mut db.pool().into(),
            )
            .await
            .unwrap();
            events.push((event, timestamp));
        }

        let test_event = &events[2].0; // Use the third event for testing
        let test_timestamp = &events[2].1;

        // Test 1: New format "timestamp:id"
        let new_format_cursor = format!("{}:{}", test_timestamp, test_event.id);
        let parsed_new = EventRepository::parse_cursor(&new_format_cursor, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(parsed_new.id, test_event.id);
        assert_eq!(parsed_new.timestamp, test_event.created_at);

        // Test 2: Legacy format - timestamp only
        let legacy_timestamp_format = test_timestamp.to_string();
        let parsed_timestamp =
            EventRepository::parse_cursor(&legacy_timestamp_format, &mut db.pool().into())
                .await
                .unwrap();
        assert_eq!(parsed_timestamp.id, test_event.id);
        assert_eq!(parsed_timestamp.timestamp, test_event.created_at);

        // Test 3: Use parsed cursors in get_by_cursor to verify they work correctly
        for (cursor_str, test_name) in [
            (new_format_cursor, "new format"),
            (legacy_timestamp_format, "legacy timestamp"),
        ] {
            let cursor = EventRepository::parse_cursor(&cursor_str, &mut db.pool().into())
                .await
                .unwrap();

            let events_after =
                EventRepository::get_by_cursor(None, Some(cursor), None, &mut db.pool().into())
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

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_cursor_display_format() {
        let timestamp = DateTime::from_timestamp(1234567890, 123456000)
            .unwrap()
            .naive_utc();
        let id = 42;
        let cursor = EventCursor::new(timestamp, id);

        let cursor_str = cursor.to_string();

        // Verify format is "timestamp:id"
        assert!(cursor_str.contains(':'));
        let parts: Vec<&str> = cursor_str.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "42"); // ID should be at the end

        // Verify the timestamp part is a valid Timestamp string
        let timestamp_part: Result<Timestamp, _> = parts[0].to_string().try_into();
        assert!(
            timestamp_part.is_ok(),
            "Timestamp part should be a valid Timestamp"
        );

        // Verify the timestamp matches what we expect
        let expected_timestamp: Timestamp = (timestamp.and_utc().timestamp_micros() as u64).into();
        assert_eq!(
            timestamp_part.unwrap().as_u64(),
            expected_timestamp.as_u64()
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_event_response_formatting() {
        use pubky_common::crypto::Hash;

        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Create event with content_hash
        let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test.txt").unwrap());
        let content_hash = Hash::from_bytes([42u8; 32]);
        let event_with_hash = EventRepository::create(
            user.id,
            EventType::Put,
            &path,
            Some(content_hash),
            &mut db.pool().into(),
        )
        .await
        .unwrap();

        // Test EventResponse with content_hash
        let response = EventResponse::from_entity(&event_with_hash);
        assert_eq!(response.event_type, EventType::Put);
        assert!(response.path.starts_with("pubky://"));
        assert!(response.path.contains("/test.txt"));
        assert!(response.content_hash.is_some());
        assert_eq!(
            response.content_hash.as_ref().unwrap(),
            &content_hash.to_hex().to_string()
        );

        // Test SSE data formatting with hash
        let sse_data = response.to_sse_data();
        assert!(sse_data.contains("pubky://"));
        assert!(sse_data.contains("cursor:"));
        assert!(sse_data.contains("content_hash:"));

        // Create event without content_hash (DELETE)
        let event_without_hash = EventRepository::create(
            user.id,
            EventType::Delete,
            &path,
            None,
            &mut db.pool().into(),
        )
        .await
        .unwrap();

        // Test EventResponse without content_hash
        let response_no_hash = EventResponse::from_entity(&event_without_hash);
        assert_eq!(response_no_hash.event_type, EventType::Delete);
        assert!(response_no_hash.content_hash.is_none());

        // Test SSE data formatting without hash
        let sse_data_no_hash = response_no_hash.to_sse_data();
        assert!(sse_data_no_hash.contains("pubky://"));
        assert!(sse_data_no_hash.contains("cursor:"));
        assert!(!sse_data_no_hash.contains("content_hash:"));
    }
}
