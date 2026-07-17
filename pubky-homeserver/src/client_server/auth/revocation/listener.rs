use std::time::Duration;

use sqlx::{postgres::PgListener, PgPool};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::Instant,
};

use super::{AuthRevocation, WireAuthRevocation, PG_AUTH_REVOCATION_CHANNEL};

/// Sized to absorb short revocation bursts without forcing unrelated streams
/// through a database revalidation round-trip.
const AUTH_REVOCATION_CHANNEL_CAPACITY: usize = 1024;
/// Bounds pre-auth subscription work while the supervisor attempts recovery.
const SUPERVISOR_MAILBOX_CAPACITY: usize = 64;
/// This keeps a Postgres outage from turning request rate into connection-attempt rate:
/// one attempt per window, and everyone else is told the listener is unavailable.
const REPLACEMENT_COOLDOWN: Duration = Duration::from_secs(1);

/// The local listener is unavailable, so accepting a private long-lived stream
/// could miss an already-committed revocation.
#[derive(Debug)]
pub(crate) struct AuthRevocationUnavailable;

type RevocationReceiver = broadcast::Receiver<AuthRevocation>;
type SubscriptionResult = Result<RevocationReceiver, AuthRevocationUnavailable>;

/// Local fan-out service for committed authentication revocations.
#[derive(Clone, Debug)]
pub(crate) struct AuthRevocationService {
    requests: mpsc::Sender<SupervisorRequest>,
}

impl AuthRevocationService {
    /// Start the supervisor with a listener already receiving, so the app never
    /// starts serving private streams without its revocation feed.
    pub(crate) async fn start(pool: &PgPool) -> Result<Self, sqlx::Error> {
        let supervisor = Supervisor {
            pool: pool.clone(),
            actor: Some(ListenerActor::spawn(pool).await?),
            replacement_cooldown: ReplacementCooldown::default(),
        };
        let (requests, mailbox) = mpsc::channel(SUPERVISOR_MAILBOX_CAPACITY);
        // The supervisor deliberately holds no clone of this service: its
        // mailbox closing is what shuts the whole chain down.
        drop(tokio::spawn(supervisor.run(mailbox)));
        Ok(Self { requests })
    }

    pub(crate) async fn subscribe(&self) -> SubscriptionResult {
        let (reply, response) = oneshot::channel();
        self.requests
            .try_send(SupervisorRequest { reply })
            .map_err(|_| AuthRevocationUnavailable)?;
        response.await.map_err(|_| AuthRevocationUnavailable)?
    }
}

struct SupervisorRequest {
    reply: oneshot::Sender<SubscriptionResult>,
}

struct ActorRequest {
    reply: oneshot::Sender<RevocationReceiver>,
}

/// Serves subscriptions from one listener actor at a time.
struct Supervisor {
    pool: PgPool,
    actor: Option<ListenerActorHandle>,
    replacement_cooldown: ReplacementCooldown,
}

#[derive(Default)]
struct ReplacementCooldown {
    last_attempt: Option<Instant>,
}

impl ReplacementCooldown {
    /// Return whether a replacement may start now. Refused attempts do not
    /// extend the current cooldown window.
    fn try_start(&mut self, now: Instant) -> bool {
        if self
            .last_attempt
            .is_some_and(|started| now.duration_since(started) < REPLACEMENT_COOLDOWN)
        {
            return false;
        }

        self.last_attempt = Some(now);
        true
    }
}

impl Supervisor {
    async fn run(mut self, mut mailbox: mpsc::Receiver<SupervisorRequest>) {
        while let Some(request) = mailbox.recv().await {
            // Retaining the reply covers an actor that exits after accepting
            // the subscription but before answering it.
            let _ = request.reply.send(self.subscribe().await);
        }
    }

    async fn subscribe(&mut self) -> SubscriptionResult {
        if let Some(handle) = self.actor.as_ref() {
            if let Some(receiver) = handle.subscribe().await {
                return Ok(receiver);
            }
            self.actor = None;
        }
        self.replace().await
    }

    async fn replace(&mut self) -> SubscriptionResult {
        if !self.replacement_cooldown.try_start(Instant::now()) {
            return Err(AuthRevocationUnavailable);
        }

        let replacement = ListenerActor::spawn(&self.pool).await.map_err(|error| {
            tracing::error!(%error, "failed to start the auth revocation listener");
            AuthRevocationUnavailable
        })?;
        let receiver = replacement
            .subscribe()
            .await
            .ok_or(AuthRevocationUnavailable)?;
        tracing::info!("started a replacement auth revocation listener");
        self.actor = Some(replacement);
        Ok(receiver)
    }
}

struct ListenerActor {
    listener: PgListener,
    notifications: broadcast::Sender<AuthRevocation>,
    mailbox: mpsc::Receiver<ActorRequest>,
}

impl ListenerActor {
    /// Connect and `LISTEN` before spawning, so a failure surfaces to the
    /// caller instead of to a stream that already believes it is protected.
    async fn spawn(pool: &PgPool) -> Result<ListenerActorHandle, sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        // `try_recv` must report a lost connection before sqlx reconnects, so
        // existing private streams can be closed before a notification gap is
        // silently accepted.
        listener.eager_reconnect(false);
        listener.listen(PG_AUTH_REVOCATION_CHANNEL).await?;

        let (notifications, _) = broadcast::channel(AUTH_REVOCATION_CHANNEL_CAPACITY);
        // The supervisor permits only one in-flight actor request.
        let (requests, mailbox) = mpsc::channel(1);
        let actor = Self {
            listener,
            notifications,
            mailbox,
        };
        drop(tokio::spawn(actor.run()));

        Ok(ListenerActorHandle { requests })
    }

    /// Forward notifications to this actor's subscribers until any gap makes
    /// the feed unsafe. Dropping the actor closes all of its receivers.
    async fn run(mut self) {
        loop {
            tokio::select! {
                request = self.mailbox.recv() => match request {
                    Some(ActorRequest { reply }) => {
                        let _ = reply.send(self.notifications.subscribe());
                    }
                    None => return,
                },
                notification = self.listener.try_recv() => match notification {
                    Ok(Some(notification)) => {
                        match serde_json::from_str::<WireAuthRevocation>(notification.payload()) {
                            Ok(revocation) => {
                                // No receivers is normal when no private streams are connected.
                                let _ = self.notifications.send(revocation.into());
                            }
                            // The payload could be a revocation this instance would
                            // otherwise ignore, so it is not safe to skip.
                            Err(error) => {
                                tracing::error!(%error, "invalid auth revocation notification; closing private streams");
                                return;
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::error!("auth revocation listener connection was lost; closing private streams");
                        return;
                    }
                    Err(error) => {
                        tracing::error!(%error, "auth revocation listener failed; closing private streams");
                        return;
                    }
                },
            }
        }
    }
}

struct ListenerActorHandle {
    requests: mpsc::Sender<ActorRequest>,
}

impl ListenerActorHandle {
    async fn subscribe(&self) -> Option<RevocationReceiver> {
        let (reply, response) = oneshot::channel();
        self.requests.send(ActorRequest { reply }).await.ok()?;
        response.await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::SqlDb;
    use tokio::time::{sleep, timeout};

    async fn receive_revocation(receiver: &mut RevocationReceiver) -> AuthRevocation {
        timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for auth revocation")
            .expect("auth revocation channel unexpectedly closed")
    }

    async fn expect_closed(receiver: &mut RevocationReceiver) {
        let result = timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for the revocation channel to close");
        assert!(
            matches!(result, Err(broadcast::error::RecvError::Closed)),
            "expected a closed revocation channel, got {result:?}"
        );
    }

    async fn notify(pool: &PgPool, payload: &str) {
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(PG_AUTH_REVOCATION_CHANNEL)
            .bind(payload)
            .execute(pool)
            .await
            .expect("send notification");
    }

    async fn notify_cookie_revocation(pool: &PgPool, id: i32) {
        let payload = serde_json::to_string(&WireAuthRevocation::CookieSession(id))
            .expect("serialize revocation");
        notify(pool, &payload).await;
    }

    async fn listener_backend_pids(pool: &PgPool) -> Vec<i32> {
        sqlx::query_scalar::<_, i32>(
            "SELECT pid FROM pg_stat_activity \
             WHERE datname = current_database() \
               AND pid <> pg_backend_pid() \
               AND query = $1",
        )
        .bind(format!("LISTEN \"{PG_AUTH_REVOCATION_CHANNEL}\""))
        .fetch_all(pool)
        .await
        .expect("query listener backends")
    }

    async fn listener_backend_pid(pool: &PgPool) -> i32 {
        timeout(Duration::from_secs(5), async {
            loop {
                if let Some(pid) = listener_backend_pids(pool).await.first() {
                    return *pid;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("timed out waiting for listener backend")
    }

    #[test]
    fn replacement_cooldown_throttles_without_extending_the_window() {
        let mut cooldown = ReplacementCooldown::default();
        let first_attempt = Instant::now();

        assert!(cooldown.try_start(first_attempt));
        assert!(!cooldown.try_start(first_attempt + REPLACEMENT_COOLDOWN - Duration::from_nanos(1)));
        assert!(cooldown.try_start(first_attempt + REPLACEMENT_COOLDOWN));
        assert!(!cooldown.try_start(first_attempt + REPLACEMENT_COOLDOWN));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn listeners_on_two_instances_receive_the_same_revocation() {
        let db = SqlDb::test().await;
        let service_a = AuthRevocationService::start(db.pool()).await.unwrap();
        let service_b = AuthRevocationService::start(db.pool()).await.unwrap();
        let mut receiver_a = service_a.subscribe().await.unwrap();
        let mut receiver_b = service_b.subscribe().await.unwrap();

        notify_cookie_revocation(db.pool(), 42).await;

        assert_eq!(
            receive_revocation(&mut receiver_a).await,
            AuthRevocation::CookieSession(42)
        );
        assert_eq!(
            receive_revocation(&mut receiver_b).await,
            AuthRevocation::CookieSession(42)
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn a_lost_connection_closes_private_streams_until_a_replacement_listener_takes_over() {
        let db = SqlDb::test().await;
        let service = AuthRevocationService::start(db.pool()).await.unwrap();
        let mut orphaned = service.subscribe().await.unwrap();

        let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
            .bind(listener_backend_pid(db.pool()).await)
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(
            terminated,
            "Postgres should terminate the listener connection"
        );

        expect_closed(&mut orphaned).await;

        let mut receiver = service
            .subscribe()
            .await
            .expect("a later subscription should start a replacement listener");

        notify_cookie_revocation(db.pool(), 42).await;
        assert_eq!(
            receive_revocation(&mut receiver).await,
            AuthRevocation::CookieSession(42)
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn an_invalid_notification_payload_closes_private_streams() {
        let db = SqlDb::test().await;
        let service = AuthRevocationService::start(db.pool()).await.unwrap();
        let mut receiver = service.subscribe().await.unwrap();

        notify(db.pool(), "not-an-auth-revocation").await;

        expect_closed(&mut receiver).await;
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn dropping_the_last_service_clone_releases_the_postgres_listener() {
        let db = SqlDb::test().await;
        let service = AuthRevocationService::start(db.pool()).await.unwrap();
        let spare = service.clone();
        let pid = listener_backend_pid(db.pool()).await;

        drop(service);
        assert!(
            spare.subscribe().await.is_ok(),
            "a remaining clone keeps the listener alive"
        );

        drop(spare);

        timeout(Duration::from_secs(5), async {
            while listener_backend_pids(db.pool()).await.contains(&pid) {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("timed out waiting for the listener connection to be released");
    }
}
