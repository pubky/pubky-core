//! Postgres LISTEN/NOTIFY event broadcaster for cross-instance event propagation.
//!
//! This module implements a background service that polls the events table for new
//! events and broadcasts them to local SSE subscribers. Postgres NOTIFY is used as
//! a wake-up hint to minimize latency, but the database is always the source of truth.
//!
//! This design guarantees sequential delivery with no gaps: events are always read
//! from the database in order, so a missed NOTIFY or listener reconnection cannot
//! cause events to be skipped.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use sqlx::{postgres::PgListener, PgPool};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::persistence::files::events::{
    EventCursor, EventRepository, EventsService, PG_NOTIFY_CHANNEL,
};
use crate::persistence::sql::UnifiedExecutor;

/// Default fallback poll interval when no NOTIFY is received.
/// This is a safety net for rare failures (missed NOTIFYs, listener downtime).
/// In the happy path, NOTIFY wakes the poll loop immediately.
const DEFAULT_FALLBACK_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Background service that polls the events table and broadcasts new events locally.
///
/// Uses Postgres NOTIFY as a wake-up hint to minimize latency. The database is
/// always the source of truth - events are read sequentially by ID, guaranteeing
/// no gaps even if NOTIFYs are lost.
pub struct PgEventListener {
    poll_handle: Option<JoinHandle<()>>,
    listen_handle: Option<JoinHandle<()>>,
}

impl PgEventListener {
    /// Start the event broadcaster.
    ///
    /// Spawns two tasks:
    /// 1. A LISTEN task that receives Postgres NOTIFY and wakes the poll loop
    /// 2. A poll loop that reads new events from the DB and broadcasts them
    ///
    /// On startup, initializes `last_broadcast_id` to the current max event ID,
    /// so only new events created after startup are broadcast.
    #[must_use = "the listener stops receiving events when dropped"]
    pub async fn start(pool: &PgPool, events_service: EventsService) -> Self {
        Self::start_with_poll_interval(pool, events_service, DEFAULT_FALLBACK_POLL_INTERVAL).await
    }

    /// Start the event broadcaster with a custom fallback poll interval.
    /// Useful for tests that need shorter timeouts.
    async fn start_with_poll_interval(
        pool: &PgPool,
        events_service: EventsService,
        fallback_poll_interval: Duration,
    ) -> Self {
        let pool = pool.clone();
        let wake = Arc::new(Notify::new());

        // Initialize last_broadcast_id to current max event ID before spawning tasks,
        // so only events created after this point are broadcast.
        let initial_id = match EventRepository::get_max_id(&mut UnifiedExecutor::from(&pool)).await
        {
            Ok(max_id) => {
                tracing::info!("PgEventListener starting, last_broadcast_id = {}", max_id);
                max_id
            }
            Err(e) => {
                tracing::error!(
                    "Failed to get max event ID on startup: {}. Starting from 0.",
                    e
                );
                0
            }
        };
        let last_broadcast_id = Arc::new(AtomicU64::new(initial_id));

        let listen_handle = {
            let pool = pool.clone();
            let wake = wake.clone();
            tokio::spawn(async move {
                Self::listen_loop(pool, wake).await;
            })
        };

        let poll_handle = {
            let wake = wake.clone();
            let last_broadcast_id = last_broadcast_id.clone();
            tokio::spawn(async move {
                Self::poll_loop(
                    pool,
                    events_service,
                    wake,
                    last_broadcast_id,
                    fallback_poll_interval,
                )
                .await;
            })
        };

        Self {
            poll_handle: Some(poll_handle),
            listen_handle: Some(listen_handle),
        }
    }

    /// Main poll loop: reads new events from DB and broadcasts them.
    /// Woken by NOTIFY hints or falls back to periodic polling.
    async fn poll_loop(
        pool: PgPool,
        events_service: EventsService,
        wake: Arc<Notify>,
        last_broadcast_id: Arc<AtomicU64>,
        fallback_poll_interval: Duration,
    ) {
        loop {
            // Wait for NOTIFY hint or timeout
            _ = tokio::time::timeout(fallback_poll_interval, wake.notified()).await;

            // Poll DB for new events
            if let Err(e) =
                Self::broadcast_new_events(&pool, &events_service, &last_broadcast_id).await
            {
                tracing::error!("Error polling events from DB: {}", e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }

    /// Query DB for events after last_broadcast_id and broadcast them in order.
    async fn broadcast_new_events(
        pool: &PgPool,
        events_service: &EventsService,
        last_broadcast_id: &AtomicU64,
    ) -> Result<(), sqlx::Error> {
        loop {
            let current_id = last_broadcast_id.load(Ordering::Relaxed);
            let cursor = EventCursor::new(current_id);

            let events = EventRepository::get_by_cursor(
                Some(cursor),
                Some(100),
                &mut UnifiedExecutor::from(pool),
            )
            .await?;

            if events.is_empty() {
                return Ok(());
            }

            for event in &events {
                events_service.broadcast_event(event.clone());
                last_broadcast_id.store(event.id, Ordering::Relaxed);
            }

            // If we got a full batch, there might be more - loop to fetch the rest.
            // Yield to let other tasks run between batches.
            tokio::task::yield_now().await;
        }
    }

    /// LISTEN loop: receives Postgres NOTIFY and wakes the poll loop.
    /// Handles reconnection on errors.
    async fn listen_loop(pool: PgPool, wake: Arc<Notify>) {
        loop {
            match Self::run_listener(&pool, &wake).await {
                Ok(()) => break,
                Err(e) => {
                    tracing::error!("PgListener error: {}. Reconnecting in 1s...", e);
                    // Wake the poll loop so it can catch up on any events missed
                    // during the listener downtime
                    wake.notify_one();
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Run the NOTIFY listener until an error occurs.
    /// Ignores the payload - just wakes the poll loop.
    async fn run_listener(pool: &PgPool, wake: &Notify) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen(PG_NOTIFY_CHANNEL).await?;

        tracing::info!("PgEventListener NOTIFY listener started");

        // Wake poll loop to catch any events created during connection setup.
        // This is critical after reconnection to fill gaps from the downtime window.
        wake.notify_one();

        loop {
            let _notification = listener.recv().await?;
            wake.notify_one();
        }
    }
}

impl Drop for PgEventListener {
    fn drop(&mut self) {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.listen_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::files::events::{EventRepository, EventType, EventsService};
    use crate::persistence::sql::SqlDb;
    use crate::shared::webdav::{EntryPath, WebDavPath};
    use pubky_common::crypto::{Hash, Keypair};
    use std::time::Duration;

    /// Helper: create a real event in the DB and send a NOTIFY to wake the listener.
    async fn create_event_and_notify(
        db: &SqlDb,
        events_service: &EventsService,
        user_id: i32,
        path: &str,
        pubkey: &pubky_common::crypto::PublicKey,
    ) -> u64 {
        let entry_path = EntryPath::new(pubkey.clone(), WebDavPath::new(path).unwrap());
        let mut tx = db.pool().begin().await.unwrap();
        let event = EventRepository::create(
            user_id,
            EventType::Put {
                content_hash: Hash::from_bytes([1; 32]),
            },
            &entry_path,
            &mut UnifiedExecutor::from(&mut tx),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        // Send NOTIFY to wake the listener (empty payload, just a hint)
        events_service.notify_event(db.pool()).await;

        event.id
    }

    /// Test that events created in DB are broadcast to subscribers via the poll loop.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_events_broadcast_from_db() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let _listener = PgEventListener::start(db.pool(), events_service.clone()).await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        let mut rx = events_service.subscribe();

        // Create 3 events in the DB
        let mut expected_ids = Vec::new();
        for i in 1..=3 {
            let id = create_event_and_notify(
                &db,
                &events_service,
                user.id,
                &format!("/pub/file{}.txt", i),
                &pubkey,
            )
            .await;
            expected_ids.push(id);
        }

        // Receive all 3 events in order
        for expected_id in &expected_ids {
            let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .expect("Timeout waiting for event")
                .expect("Channel closed");
            assert_eq!(received.id, *expected_id);
        }
    }

    /// Test that two instances sharing the same DB both receive all events.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_cross_instance_propagation() {
        let db = SqlDb::test().await;

        let events_service_a = EventsService::new(100);
        let _listener_a = PgEventListener::start(db.pool(), events_service_a.clone()).await;

        let events_service_b = EventsService::new(100);
        let _listener_b = PgEventListener::start(db.pool(), events_service_b.clone()).await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        let mut rx_a = events_service_a.subscribe();
        let mut rx_b = events_service_b.subscribe();

        // Instance A writes an event
        let event_id =
            create_event_and_notify(&db, &events_service_a, user.id, "/pub/from-a.txt", &pubkey)
                .await;

        // Both instances should receive it
        let recv_a = tokio::time::timeout(Duration::from_secs(5), rx_a.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");
        let recv_b = tokio::time::timeout(Duration::from_secs(5), rx_b.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");

        assert_eq!(recv_a.id, event_id, "Instance A should receive the event");
        assert_eq!(recv_b.id, event_id, "Instance B should receive the event");
    }

    /// Helper: create an event in the DB without sending NOTIFY.
    async fn create_event_silent(
        db: &SqlDb,
        user_id: i32,
        path: &str,
        pubkey: &pubky_common::crypto::PublicKey,
    ) -> u64 {
        let entry_path = EntryPath::new(pubkey.clone(), WebDavPath::new(path).unwrap());
        let mut tx = db.pool().begin().await.unwrap();
        let event = EventRepository::create(
            user_id,
            EventType::Put {
                content_hash: Hash::from_bytes([1; 32]),
            },
            &entry_path,
            &mut UnifiedExecutor::from(&mut tx),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        event.id
    }

    /// Test that events are eventually delivered even without NOTIFY (fallback polling).
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_fallback_polling_without_notify() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        // Start listener with a short poll interval for faster testing
        let _listener = PgEventListener::start_with_poll_interval(
            db.pool(),
            events_service.clone(),
            Duration::from_secs(1),
        )
        .await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        // Small delay to let the listener initialize
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut rx = events_service.subscribe();

        // Create event in DB but do NOT send NOTIFY
        let event_id = create_event_silent(&db, user.id, "/pub/silent.txt", &pubkey).await;

        // The event should still be delivered via fallback polling (1s interval in this test).
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout - fallback polling did not deliver the event")
            .expect("Channel closed");

        assert_eq!(received.id, event_id);
    }

    /// Test that get_max_id returns 0 on an empty table and the correct ID after inserts.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_get_max_id() {
        let db = SqlDb::test().await;

        // Empty table should return 0
        let max_id = EventRepository::get_max_id(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(max_id, 0);

        // Insert some events
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        let mut last_id = 0;
        for i in 1..=3 {
            last_id =
                create_event_silent(&db, user.id, &format!("/pub/file{}.txt", i), &pubkey).await;
        }

        let max_id = EventRepository::get_max_id(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(max_id, last_id);
    }

    /// Test that events created before the listener starts are NOT broadcast.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_pre_existing_events_not_broadcast() {
        let db = SqlDb::test().await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        // Create events BEFORE starting the listener
        for i in 1..=5 {
            create_event_silent(&db, user.id, &format!("/pub/old{}.txt", i), &pubkey).await;
        }

        let events_service = EventsService::new(100);
        let _listener = PgEventListener::start(db.pool(), events_service.clone()).await;
        let mut rx = events_service.subscribe();

        // Create one NEW event after the listener started
        let new_id =
            create_event_and_notify(&db, &events_service, user.id, "/pub/new.txt", &pubkey).await;

        // Should only receive the new event, not the 5 pre-existing ones
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");
        assert_eq!(received.id, new_id);
    }

    /// Test that dropping a listener and starting a new one correctly catches up.
    /// Simulates: instance crash → events produced during downtime → instance restart.
    /// The new listener should skip gap events (they're already in the DB for cursor-based
    /// SSE catch-up) and only broadcast events created after restart.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_listener_restart_skips_gap_and_resumes() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        // Phase 1: Start listener, verify it works
        let listener = PgEventListener::start(db.pool(), events_service.clone()).await;
        let mut rx = events_service.subscribe();

        let initial_id = create_event_and_notify(
            &db,
            &events_service,
            user.id,
            "/pub/before-crash.txt",
            &pubkey,
        )
        .await;

        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");
        assert_eq!(received.id, initial_id);

        // Phase 2: Drop listener (simulates crash)
        drop(listener);
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Phase 3: Create events during the gap (no listener running)
        let mut gap_ids = Vec::new();
        for i in 1..=3 {
            let id =
                create_event_silent(&db, user.id, &format!("/pub/during-gap{}.txt", i), &pubkey)
                    .await;
            gap_ids.push(id);
        }

        // Phase 4: Start a NEW listener (simulates instance restart)
        let _listener2 = PgEventListener::start(db.pool(), events_service.clone()).await;

        // Phase 5: Create a new event after restart
        let post_restart_id = create_event_and_notify(
            &db,
            &events_service,
            user.id,
            "/pub/after-restart.txt",
            &pubkey,
        )
        .await;

        // Phase 6: The subscriber should receive ONLY the post-restart event.
        // Gap events are skipped because the new listener initialized last_broadcast_id
        // to the DB max (which includes the gap events).
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout waiting for post-restart event")
            .expect("Channel closed");
        assert_eq!(
            received.id, post_restart_id,
            "Should receive the post-restart event, not a gap event"
        );
    }

    /// Test that gap events are delivered when the NOTIFY listener reconnects internally.
    /// Simulates the scenario where the Postgres LISTEN connection drops momentarily
    /// but the PgEventListener stays alive. The poll loop's fallback timer should
    /// catch events produced while NOTIFY was unavailable.
    ///
    /// We approximate this by:
    /// 1. Starting a listener with a short fallback poll interval
    /// 2. Creating events without NOTIFY (simulating lost notifications)
    /// 3. Verifying they arrive via fallback polling with no gaps
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_internal_reconnect_catches_gap_events() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let _listener = PgEventListener::start_with_poll_interval(
            db.pool(),
            events_service.clone(),
            Duration::from_secs(1),
        )
        .await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        let mut rx = events_service.subscribe();

        // First, deliver an event normally to advance last_broadcast_id
        let first_id = create_event_and_notify(
            &db,
            &events_service,
            user.id,
            "/pub/before-gap.txt",
            &pubkey,
        )
        .await;
        let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");
        assert_eq!(received.id, first_id);

        // Now create events WITHOUT notify (simulating lost notifications during reconnect)
        let mut gap_ids = Vec::new();
        for i in 1..=3 {
            let id =
                create_event_silent(&db, user.id, &format!("/pub/gap{}.txt", i), &pubkey).await;
            gap_ids.push(id);
        }

        // All gap events should arrive via fallback polling, in order, with no gaps
        for (idx, expected_id) in gap_ids.iter().enumerate() {
            let received = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "Timeout waiting for gap event {}/{}",
                        idx + 1,
                        gap_ids.len()
                    )
                })
                .expect("Channel closed");
            assert_eq!(
                received.id,
                *expected_id,
                "Gap event {} should be delivered in order",
                idx + 1
            );
        }
    }

    /// Test that the batch loop handles more than 100 events correctly.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_batch_boundary_over_100_events() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(250);

        let _listener = PgEventListener::start(db.pool(), events_service.clone()).await;

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user =
            crate::persistence::sql::user::UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();

        let mut rx = events_service.subscribe();

        // Create 150 events (exceeds the batch size of 100)
        let total = 150;
        let mut expected_ids = Vec::new();
        for i in 1..=total {
            let id =
                create_event_silent(&db, user.id, &format!("/pub/batch{}.txt", i), &pubkey).await;
            expected_ids.push(id);
        }

        // Send a single NOTIFY to wake the poll loop
        events_service.notify_event(db.pool()).await;

        // All 150 events should be delivered in order
        for (idx, expected_id) in expected_ids.iter().enumerate() {
            let received = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("Timeout waiting for event {}/{}", idx + 1, total))
                .expect("Channel closed");
            assert_eq!(
                received.id,
                *expected_id,
                "Event {} out of order: expected {}, got {}",
                idx + 1,
                expected_id,
                received.id
            );
        }
    }
}
