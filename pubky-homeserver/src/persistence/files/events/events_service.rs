use crate::persistence::{
    files::events::{events_repository::EventCursor, EventEntity, EventRepository, EventType},
    sql::UnifiedExecutor,
};
use crate::shared::webdav::EntryPath;
use tokio::sync::broadcast;

/// Maximum number of users allowed in a single event stream request.
/// Based on HTTP header size limits (~4KB) and typical URL encoding:
/// - Max users at 4KB: 3896 / 74 ≈ 52 users
/// - Set to 50 for clean limit with safety margin for longer cursors
pub const MAX_EVENT_STREAM_USERS: usize = 50;

/// Service that handles all event-related business logic.
#[derive(Clone, Debug)]
pub struct EventsService {
    event_tx: broadcast::Sender<EventEntity>,
}

impl EventsService {
    /// Create a new EventsService with a broadcast channel.
    /// The channel_capacity determines how many events can be buffered before old ones are dropped.
    pub fn new(channel_capacity: usize) -> Self {
        let (event_tx, _rx) = broadcast::channel(channel_capacity);
        Self { event_tx }
    }

    /// Subscribe to the event broadcast channel.
    /// Returns a receiver that will receive all future events.
    pub fn subscribe(&self) -> broadcast::Receiver<EventEntity> {
        self.event_tx.subscribe()
    }

    /// Create a new event in the database.
    /// The event will be returned but NOT broadcast - use `broadcast_event` after transaction commit.
    ///
    /// ## Usage Pattern
    /// ```rust,ignore
    /// let mut tx = db.pool().begin().await?;
    /// let event = events_service.create_event(..., &mut (&mut tx).into()).await?;
    /// tx.commit().await?;
    /// events_service.broadcast_event(event);
    /// ```
    pub async fn create_event<'a>(
        &self,
        user_id: i32,
        event_type: EventType,
        path: &EntryPath,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventEntity, sqlx::Error> {
        EventRepository::create(user_id, event_type, path, executor).await
    }

    /// Broadcast an event to all subscribers.
    /// This should be called AFTER the database transaction has been committed.
    ///
    /// ## Timing
    /// It's critical to broadcast only after commit to avoid race conditions where
    /// subscribers receive events that don't exist in the database yet.
    pub fn broadcast_event(&self, event: EventEntity) {
        match self.event_tx.send(event) {
            Ok(_) => {} // Successfully broadcast to receivers
            Err(broadcast::error::SendError(_)) => {
                // No active receivers - this is expected when no clients are listening
            }
        }
    }

    /// Parse a cursor string into a Cursor object.
    /// Supports both new cursor format (event ID) and legacy format (timestamp).
    pub async fn parse_cursor<'a>(
        &self,
        cursor: &str,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<EventCursor, sqlx::Error> {
        EventRepository::parse_cursor(cursor, executor).await
    }

    /// Get a list of events starting from a cursor position.
    /// This is used by the `/events/` endpoint.
    ///
    /// ## Parameters
    /// - `cursor`: Starting position (None = from beginning)
    /// - `limit`: Maximum number of events to return (None = default limit)
    pub async fn get_by_cursor<'a>(
        &self,
        cursor: Option<EventCursor>,
        limit: Option<u16>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        EventRepository::get_by_cursor(cursor, limit, executor).await
    }

    /// Get events for multiple users with individual cursor positions.
    ///
    /// ## Parameters
    /// - `user_cursors`: Vec of (user_id, optional_cursor) pairs
    /// - `reverse`: If true, return newest events first
    /// - `path_prefix`: Optional path filter (e.g., "/pub/files/")
    pub async fn get_by_user_cursors<'a>(
        &self,
        user_cursors: Vec<(i32, Option<EventCursor>)>,
        reverse: bool,
        path_prefix: Option<&str>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<EventEntity>, sqlx::Error> {
        EventRepository::get_by_user_cursors(user_cursors, reverse, path_prefix, executor).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::{user::UserRepository, SqlDb};
    use crate::shared::webdav::WebDavPath;
    use pkarr::Keypair;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_events_service_create_and_broadcast() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let user_pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let path = EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test.txt").unwrap());

        // Subscribe before creating event
        let mut rx = events_service.subscribe();

        // Create event within transaction
        let mut tx = db.pool().begin().await.unwrap();
        let event = events_service
            .create_event(
                user.id,
                EventType::Put {
                    content_hash: pubky_common::crypto::Hash::from_bytes([0; 32]),
                },
                &path,
                &mut (&mut tx).into(),
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();

        // Broadcast after commit
        events_service.broadcast_event(event.clone());

        // Verify broadcast received
        let received = rx.recv().await.unwrap();
        assert_eq!(received.id, event.id);
        assert_eq!(received.user_id, user.id);
        assert!(matches!(received.event_type, EventType::Put { .. }));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_events_service_get_by_cursor() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let user_pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Create multiple events
        for i in 0..5 {
            let path = EntryPath::new(
                user_pubkey.clone(),
                WebDavPath::new(&format!("/test{}.txt", i)).unwrap(),
            );
            events_service
                .create_event(
                    user.id,
                    EventType::Put {
                        content_hash: pubky_common::crypto::Hash::from_bytes([0; 32]),
                    },
                    &path,
                    &mut db.pool().into(),
                )
                .await
                .unwrap();
        }

        // Get events from cursor
        let events = events_service
            .get_by_cursor(Some(EventCursor::new(2)), Some(3), &mut db.pool().into())
            .await
            .unwrap();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, 3);
        assert_eq!(events[1].id, 4);
        assert_eq!(events[2].id, 5);
    }
}
