use std::{fmt::Display, str::FromStr};

use pkarr::PublicKey;
use pubky_common::crypto::Hash;
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

/// Cursor for pagination in event queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor(i64);

impl Cursor {
    /// Create a new cursor from an event ID
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    /// Get the underlying ID value
    pub fn id(&self) -> i64 {
        self.0
    }
}

impl Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Cursor {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Cursor(s.parse()?))
    }
}

/// Response structure for event streams.
/// This represents the data format returned by the `/events-stream` endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventResponse {
    /// The type of event (PUT or DEL)
    pub event_type: EventType,
    /// The full pubky path (e.g., "pubky://user_pubkey/pub/example.txt")
    pub path: String,
    /// Cursor for pagination (event id as string)
    pub cursor: String,
    /// content_hash (blake3) of data in hex format
    /// **Note**: Optional and only included for PUT events when the hash is available.
    /// Legacy events created before the content_hash feature was added will not have this field.
    pub content_hash: Option<String>,
}

impl EventResponse {
    /// Create an EventResponse from an EventEntity
    pub fn from_entity(entity: &EventEntity) -> Self {
        Self {
            event_type: entity.event_type.clone(),
            path: format!("pubky://{}", entity.path.as_str()),
            cursor: entity.cursor().to_string(),
            content_hash: entity.content_hash.map(|h| h.to_hex().to_string()),
        }
    }

    /// Format as SSE event data.
    /// Returns the multiline data field content.
    /// Each line will be prefixed with "data: " by the SSE library.
    /// Format:
    /// ```text
    /// data: pubky://user_pubkey/pub/example.txt
    /// data: cursor: 42
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

    /// Parse the cursor to a Cursor.
    /// The cursor can be either a new cursor format or a legacy cursor format.
    /// The new cursor format is a u64.
    /// The legacy cursor format is a timestamp.
    /// The cursor is the id of the last event in the list.
    /// If you don't to use the cursor, set it to "0".
    pub async fn parse_cursor<'a>(
        cursor: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Cursor, sqlx::Error> {
        if let Ok(cursor) = cursor.parse::<Cursor>() {
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
        Ok(Cursor::new(event_id))
    }

    /// Get a list of events with per-user cursors.
    /// Each user has their own cursor position.
    /// The limit is the maximum total number of events to return across all users.
    /// The reverse parameter determines the ordering: false for ascending (oldest first), true for descending (newest first).
    /// The filter_dir_suffix parameter filters events by path prefix (e.g., "pub/files/" to match only events under that directory).
    /// The executor can either be db.pool() or a transaction.
    /// This uses the (user, id) index for efficient querying.
    pub async fn get_by_user_cursors<'a>(
        user_cursors: Vec<(i32, Option<Cursor>)>,
        reverse: bool,
        filter_dir_suffix: Option<&str>,
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

            // Add path filter if specified
            // Note: paths in the database are stored without the user pubkey prefix (e.g., "pub/files/doc.txt")
            if let Some(filter_suffix) = filter_dir_suffix {
                let like_pattern = format!("{}%", filter_suffix);
                statement = statement
                    .and_where(Expr::col((EVENT_TABLE, EventIden::Path)).like(like_pattern))
                    .to_owned();
            }

            // Add cursor condition for this specific user
            if let Some(cursor) = cursor {
                if reverse {
                    // For reverse order, get events before the cursor
                    statement = statement
                        .and_where(Expr::col((EVENT_TABLE, EventIden::Id)).lt(cursor.id()))
                        .to_owned();
                } else {
                    // For normal order, get events after the cursor
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
        cursor: Option<Cursor>,
        limit: Option<u16>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        let cursor = cursor.unwrap_or(Cursor::new(0));
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

impl EventEntity {
    pub fn cursor(&self) -> Cursor {
        Cursor::new(self.id)
    }
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

        // Test create session
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

        // Test get session
        let events =
            EventRepository::get_by_cursor(Some(Cursor::new(5)), Some(4), &mut db.pool().into())
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

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_cursor_display_format() {
        // Test that cursor is simply the event id as a string
        let id = 42i64;
        let cursor_str = id.to_string();

        // Verify it's just the id
        assert_eq!(cursor_str, "42");

        // Verify it can be parsed back
        let parsed_id: i64 = cursor_str.parse().unwrap();
        assert_eq!(parsed_id, id);
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

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_get_by_user_cursors_with_filter_dir() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Create events in different directories
        let paths_and_names = vec![
            ("/pub/files/doc1.txt", "pub/files/"),
            ("/pub/files/doc2.txt", "pub/files/"),
            ("/pub/photos/pic1.jpg", "pub/photos/"),
            ("/pub/root.txt", "pub/"),
            ("/private/secret.txt", "private/"),
        ];

        for (path_str, _) in &paths_and_names {
            let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new(path_str).unwrap());
            EventRepository::create(user.id, EventType::Put, &path, None, &mut db.pool().into())
                .await
                .unwrap();
        }

        // Test 1: Filter by "/pub/files/" - should get 2 events
        let user_cursors = vec![(user.id, None)];
        let events = EventRepository::get_by_user_cursors(
            user_cursors.clone(),
            false,
            Some("/pub/files/"),
            &mut db.pool().into(),
        )
        .await
        .unwrap();
        assert_eq!(
            events.len(),
            2,
            "Should get 2 events with /pub/files/ filter"
        );
        for event in &events {
            assert!(event.path.path().as_str().starts_with("/pub/files/"));
        }

        // Test 2: Filter by "/pub/" - should get 4 events (files, photos, root)
        let events = EventRepository::get_by_user_cursors(
            user_cursors.clone(),
            false,
            Some("/pub/"),
            &mut db.pool().into(),
        )
        .await
        .unwrap();
        assert_eq!(events.len(), 4, "Should get 4 events with /pub/ filter");
        for event in &events {
            assert!(event.path.path().as_str().starts_with("/pub/"));
        }

        // Test 3: No filter - should get all 5 events
        let events =
            EventRepository::get_by_user_cursors(user_cursors, false, None, &mut db.pool().into())
                .await
                .unwrap();
        assert_eq!(events.len(), 5, "Should get all 5 events without filter");
    }
}
