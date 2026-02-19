//! Postgres LISTEN/NOTIFY event listener for cross-instance event propagation.
//!
//! This module implements a background service that listens for event notifications
//! from Postgres and broadcasts them to local SSE subscribers. This enables
//! horizontal scaling where events written on any instance are delivered to
//! clients connected to any other instance.

use std::time::Duration;

use sqlx::{postgres::PgListener, PgPool};
use tokio::task::JoinHandle;

use crate::persistence::files::events::{EventEntity, EventsService};

/// Background service that listens for Postgres NOTIFY events and broadcasts them locally.
///
/// When an event is written to the database, `notify_event` sends a Postgres NOTIFY.
/// This listener receives those notifications and forwards them to the local
/// broadcast channel, ensuring all instances receive all events.
pub struct PgEventListener {
    handle: Option<JoinHandle<()>>,
}

impl PgEventListener {
    /// Start the Postgres event listener.
    ///
    /// Listens on the "events" channel and forwards received events to the
    /// broadcast channel via `EventsService::broadcast_event`.
    #[must_use = "the listener must be kept alive to receive events"]
    pub fn start(pool: &PgPool, events_service: EventsService) -> Self {
        let pool = pool.clone();
        let handle = tokio::spawn(async move {
            Self::listen_loop(pool, events_service).await;
        });
        Self {
            handle: Some(handle),
        }
    }

    /// Main loop that handles reconnection on errors.
    async fn listen_loop(pool: PgPool, events_service: EventsService) {
        loop {
            match Self::run_listener(&pool, &events_service).await {
                Ok(()) => break, // Clean shutdown (should not happen in normal operation)
                Err(e) => {
                    tracing::error!("PgListener error: {}. Reconnecting in 1s...", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Run the listener until an error occurs.
    async fn run_listener(
        pool: &PgPool,
        events_service: &EventsService,
    ) -> Result<(), sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        listener.listen("events").await?;

        tracing::info!("PgEventListener started, listening for events");

        loop {
            let notification = listener.recv().await?;
            match serde_json::from_str::<EventEntity>(notification.payload()) {
                Ok(event) => {
                    events_service.broadcast_event(event);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to deserialize event notification: {}. Payload: {}",
                        e,
                        notification.payload()
                    );
                }
            }
        }
    }
}

impl Drop for PgEventListener {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::SqlDb;
    use crate::shared::webdav::{EntryPath, WebDavPath};
    use chrono::NaiveDateTime;
    use pubky_common::crypto::{Hash, Keypair};
    use pubky_common::events::EventType;
    use std::time::Duration;
    use tokio::sync::broadcast;

    /// Helper to wait for listener to be ready by sending a probe event and waiting for it.
    /// Retries with exponential backoff if the listener isn't ready yet.
    async fn wait_for_listener_ready(
        events_service: &EventsService,
        pool: &PgPool,
        mut rx: broadcast::Receiver<EventEntity>,
    ) {
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let probe_event = EventEntity {
            id: 0,
            user_id: 0,
            user_pubkey: pubkey.clone(),
            event_type: EventType::Delete,
            path: EntryPath::new(pubkey, WebDavPath::new("/probe").unwrap()),
            created_at: NaiveDateTime::parse_from_str("2000-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        for attempt in 0..5 {
            events_service.notify_event(&probe_event, pool).await;
            match tokio::time::timeout(Duration::from_millis(100 << attempt), rx.recv()).await {
                Ok(Ok(_)) => return,
                _ => continue,
            }
        }
        panic!("Listener failed to become ready after 5 attempts");
    }

    /// Test that pg_notify sends an event and PgEventListener receives it
    /// via the broadcast channel.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_pg_notify_roundtrip() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        // Start the listener
        let _listener = PgEventListener::start(db.pool(), events_service.clone());

        // Wait for listener to be ready using probe event
        let probe_rx = events_service.subscribe();
        wait_for_listener_ready(&events_service, db.pool(), probe_rx).await;

        // Subscribe to the broadcast channel
        let mut rx = events_service.subscribe();

        // Create a test event
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/test.txt").unwrap());
        let event = EventEntity {
            id: 12345,
            user_id: 42,
            user_pubkey: pubkey,
            event_type: EventType::Put {
                content_hash: Hash::from_bytes([1; 32]),
            },
            path,
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        // Send via pg_notify (simulating what notify_event does)
        events_service.notify_event(&event, db.pool()).await;

        // Wait for the event to come through the listener
        let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("Channel closed");

        assert_eq!(received.id, event.id);
        assert_eq!(received.user_id, event.user_id);
        assert_eq!(received.user_pubkey, event.user_pubkey);
        assert_eq!(received.event_type, event.event_type);
        assert_eq!(received.path, event.path);
    }

    /// Test that multiple events are received in order.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_pg_notify_multiple_events() {
        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);

        let _listener = PgEventListener::start(db.pool(), events_service.clone());

        // Wait for listener to be ready
        let probe_rx = events_service.subscribe();
        wait_for_listener_ready(&events_service, db.pool(), probe_rx).await;

        let mut rx = events_service.subscribe();

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        // Send 3 events
        for i in 1..=3 {
            let path = EntryPath::new(
                pubkey.clone(),
                WebDavPath::new(&format!("/pub/file{}.txt", i)).unwrap(),
            );
            let event = EventEntity {
                id: i as u64,
                user_id: 1,
                user_pubkey: pubkey.clone(),
                event_type: EventType::Put {
                    content_hash: Hash::from_bytes([i as u8; 32]),
                },
                path,
                created_at: NaiveDateTime::parse_from_str(
                    "2024-01-15 10:30:00",
                    "%Y-%m-%d %H:%M:%S",
                )
                .unwrap(),
            };
            events_service.notify_event(&event, db.pool()).await;
        }

        // Receive all 3 events
        for expected_id in 1..=3 {
            let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("Timeout waiting for event")
                .expect("Channel closed");

            assert_eq!(received.id, expected_id as u64);
        }
    }

    /// Simulate two homeserver instances sharing the same Postgres database.
    /// Tests bidirectional cross-instance communication:
    /// - Instance A writes → both A and B receive via pg_notify
    /// - Instance B writes → both A and B receive via pg_notify
    ///
    /// Note: Full e2e testing was difficult due to postgres's test db design which
    /// creates separate ephemeral databases per homeserver. Instead, we instantiate
    /// only the EventsService + PgEventListener components sharing a single db pool.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_bidirectional_cross_instance_propagation() {
        let db = SqlDb::test().await;

        // Two instances
        let events_service_a = EventsService::new(100);
        let _listener_a = PgEventListener::start(db.pool(), events_service_a.clone());

        let events_service_b = EventsService::new(100);
        let _listener_b = PgEventListener::start(db.pool(), events_service_b.clone());

        // Wait for both listeners to be ready
        let probe_rx_a = events_service_a.subscribe();
        wait_for_listener_ready(&events_service_a, db.pool(), probe_rx_a).await;
        let probe_rx_b = events_service_b.subscribe();
        wait_for_listener_ready(&events_service_b, db.pool(), probe_rx_b).await;

        // Subscribe to both instances
        let mut rx_a = events_service_a.subscribe();
        let mut rx_b = events_service_b.subscribe();

        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        // Event 1: Written by Instance A
        let event_from_a = EventEntity {
            id: 1,
            user_id: 1,
            user_pubkey: pubkey.clone(),
            event_type: EventType::Put {
                content_hash: Hash::from_bytes([1; 32]),
            },
            path: EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/from-a.txt").unwrap()),
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:30:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        // Event 2: Written by Instance B
        let event_from_b = EventEntity {
            id: 2,
            user_id: 1,
            user_pubkey: pubkey.clone(),
            event_type: EventType::Delete,
            path: EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/from-b.txt").unwrap()),
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:31:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        };

        // Instance A writes event 1
        events_service_a
            .notify_event(&event_from_a, db.pool())
            .await;

        // Both instances should receive event 1
        let recv_a_1 = tokio::time::timeout(Duration::from_secs(2), rx_a.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");
        let recv_b_1 = tokio::time::timeout(Duration::from_secs(2), rx_b.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");

        assert_eq!(recv_a_1.id, 1, "Instance A should receive event from A");
        assert_eq!(recv_b_1.id, 1, "Instance B should receive event from A");

        // Instance B writes event 2
        events_service_b
            .notify_event(&event_from_b, db.pool())
            .await;

        // Both instances should receive event 2
        let recv_a_2 = tokio::time::timeout(Duration::from_secs(2), rx_a.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");
        let recv_b_2 = tokio::time::timeout(Duration::from_secs(2), rx_b.recv())
            .await
            .expect("Timeout")
            .expect("Channel closed");

        assert_eq!(recv_a_2.id, 2, "Instance A should receive event from B");
        assert_eq!(recv_b_2.id, 2, "Instance B should receive event from B");
    }
}
