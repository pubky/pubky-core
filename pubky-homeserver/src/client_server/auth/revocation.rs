//! Cross-instance notification of authentication revocations.
//!
//! A private SSE subscription authorizes once when it is opened, so a normal
//! request-time auth check is not enough to stop it after its credential is
//! revoked. This module forwards Postgres `LISTEN`/`NOTIFY` messages to a local
//! broadcast channel that those subscriptions can observe.
//!
//! This listener intentionally has its own connection instead of sharing the
//! file-event listener. File events have a durable database catch-up path;
//! auth revocations do not, and therefore must fail closed on any listener
//! gap. Sharing lifecycle and failure handling would make an event-listener
//! disruption unnecessarily disconnect every private stream.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use pubky_common::auth::jws::GrantId;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgListener, PgPool};
use tokio::{sync::broadcast, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::persistence::sql::UnifiedExecutor;

use super::AuthSession;

/// Postgres channel used for committed authentication revocations.
const PG_AUTH_REVOCATION_CHANNEL: &str = "auth_revocations";
/// Sized to absorb short revocation bursts without forcing unrelated streams
/// through a database revalidation round-trip.
const AUTH_REVOCATION_CHANNEL_CAPACITY: usize = 1024;

/// A local revocation signal for an active private stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AuthRevocation {
    /// A deprecated cookie session row was deleted.
    CookieSession(i32),
    /// A grant and all of its bearer sessions were revoked.
    Grant(GrantId),
    /// The listener cannot guarantee that no notification was missed.
    ///
    /// This is intentionally local-only and never sent over Postgres.
    All,
}

impl AuthRevocation {
    /// Return whether this signal invalidates `session`.
    pub(crate) fn matches(&self, session: &AuthSession) -> bool {
        match (self, session) {
            (Self::All, _) => true,
            (Self::CookieSession(id), AuthSession::Cookie(cookie)) => id == &cookie.id,
            (Self::Grant(id), AuthSession::Grant(grant)) => id == &grant.grant_id,
            _ => false,
        }
    }

    /// Queue a cookie-session revocation in the caller's transaction.
    pub(crate) async fn notify_cookie_session_in_transaction<'a>(
        id: i32,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        WireAuthRevocation::CookieSession(id)
            .notify_in_transaction(executor)
            .await
    }

    /// Queue a grant revocation in the caller's transaction.
    pub(crate) async fn notify_grant_in_transaction<'a>(
        id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        WireAuthRevocation::Grant(id.clone())
            .notify_in_transaction(executor)
            .await
    }
}

/// Serializable form of an authentication revocation.
///
/// Cookie secrets and bearer tokens are deliberately not present in this
/// payload. Postgres channel consumers only need stable database identifiers.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
enum WireAuthRevocation {
    CookieSession(i32),
    Grant(GrantId),
}

impl WireAuthRevocation {
    /// Queue this notification in the caller's transaction.
    ///
    /// Postgres only delivers a `NOTIFY` at commit. Keeping this alongside the
    /// database mutation means a revocation cannot commit without its shutdown
    /// signal, and a rolled-back mutation never closes streams.
    async fn notify_in_transaction<'a>(
        &self,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let payload =
            serde_json::to_string(self).expect("auth revocation payload is always serializable");
        let con = executor.get_con().await?;
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(PG_AUTH_REVOCATION_CHANNEL)
            .bind(payload)
            .execute(con)
            .await?;
        Ok(())
    }
}

impl From<WireAuthRevocation> for AuthRevocation {
    fn from(value: WireAuthRevocation) -> Self {
        match value {
            WireAuthRevocation::CookieSession(id) => Self::CookieSession(id),
            WireAuthRevocation::Grant(id) => Self::Grant(id),
        }
    }
}

#[derive(Debug)]
struct AuthRevocationInner {
    sender: broadcast::Sender<AuthRevocation>,
    listener_healthy: AtomicBool,
}

/// Local fan-out service for committed authentication revocations.
#[derive(Clone, Debug)]
pub(crate) struct AuthRevocationService {
    inner: Arc<AuthRevocationInner>,
}

impl AuthRevocationService {
    pub(crate) fn new() -> Self {
        let (sender, _) = broadcast::channel(AUTH_REVOCATION_CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(AuthRevocationInner {
                sender,
                listener_healthy: AtomicBool::new(false),
            }),
        }
    }

    /// Subscribe before checking health so a listener failure cannot be missed
    /// in the gap between the two operations.
    pub(crate) fn subscribe(
        &self,
    ) -> Result<broadcast::Receiver<AuthRevocation>, AuthRevocationUnavailable> {
        let receiver = self.inner.sender.subscribe();
        if self.inner.listener_healthy.load(Ordering::Acquire) {
            Ok(receiver)
        } else {
            Err(AuthRevocationUnavailable)
        }
    }

    fn broadcast(&self, revocation: AuthRevocation) {
        // No receivers is normal when no private streams are connected.
        let _ = self.inner.sender.send(revocation);
    }

    fn mark_healthy(&self) {
        self.inner.listener_healthy.store(true, Ordering::Release);
    }

    /// Stop all existing private streams before reconnecting. New private
    /// streams are rejected until the next successful `LISTEN`.
    fn mark_unhealthy(&self) {
        self.inner.listener_healthy.store(false, Ordering::Release);
        self.broadcast(AuthRevocation::All);
    }
}

/// The local listener is unavailable, so accepting a private long-lived stream
/// could miss an already-committed revocation.
#[derive(Debug)]
pub(crate) struct AuthRevocationUnavailable;

/// Background Postgres listener for [`AuthRevocationService`].
pub(crate) struct PgAuthRevocationListener {
    handle: Option<JoinHandle<()>>,
    cancel: CancellationToken,
}

impl PgAuthRevocationListener {
    /// Connect and issue `LISTEN` before returning so the app never starts
    /// serving private streams without its revocation feed.
    pub(crate) async fn start(
        pool: &PgPool,
        service: AuthRevocationService,
    ) -> Result<Self, sqlx::Error> {
        let listener = Self::connect(pool).await?;
        service.mark_healthy();

        let cancel = CancellationToken::new();
        let handle = {
            let pool = pool.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move {
                Self::listen_loop(pool, service, listener, cancel).await;
            })
        };

        Ok(Self {
            handle: Some(handle),
            cancel,
        })
    }

    async fn connect(pool: &PgPool) -> Result<PgListener, sqlx::Error> {
        let mut listener = PgListener::connect_with(pool).await?;
        // `try_recv` must report a lost connection before sqlx reconnects, so
        // existing private streams can be closed before a notification gap is
        // silently accepted.
        listener.eager_reconnect(false);
        listener.listen(PG_AUTH_REVOCATION_CHANNEL).await?;
        Ok(listener)
    }

    async fn listen_loop(
        pool: PgPool,
        service: AuthRevocationService,
        mut listener: PgListener,
        cancel: CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                notification = listener.try_recv() => match notification {
                    Ok(Some(notification)) => {
                        match serde_json::from_str::<WireAuthRevocation>(notification.payload()) {
                            Ok(revocation) => service.broadcast(revocation.into()),
                            Err(error) => {
                                tracing::error!(%error, "invalid auth revocation notification; closing private streams");
                                service.broadcast(AuthRevocation::All);
                            }
                        }
                    }
                    Ok(None) => {
                        tracing::error!("auth revocation listener connection was lost; closing private streams");
                        service.mark_unhealthy();
                        listener = match Self::reconnect(&pool, &service, &cancel).await {
                            Some(listener) => listener,
                            None => return,
                        };
                    }
                    Err(error) => {
                        tracing::error!(%error, "auth revocation listener failed; closing private streams");
                        service.mark_unhealthy();
                        listener = match Self::reconnect(&pool, &service, &cancel).await {
                            Some(listener) => listener,
                            None => return,
                        };
                    }
                }
            }
        }
    }

    async fn reconnect(
        pool: &PgPool,
        service: &AuthRevocationService,
        cancel: &CancellationToken,
    ) -> Option<PgListener> {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return None,
                result = Self::connect(pool) => match result {
                    Ok(listener) => {
                        tracing::info!("auth revocation listener reconnected");
                        service.mark_healthy();
                        return Some(listener);
                    }
                    Err(error) => {
                        tracing::error!(%error, "failed to reconnect auth revocation listener; retrying in 1s");
                    }
                }
            }

            tokio::select! {
                _ = cancel.cancelled() => return None,
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            }
        }
    }
}

impl Drop for PgAuthRevocationListener {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client_server::auth::{
        cookie::persistence::{SessionEntity, SessionSecret},
        grant::session::GrantSession,
    };
    use crate::persistence::sql::SqlDb;
    use pubky_common::{capabilities::Capabilities, crypto::Keypair};
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn cookie_session(id: i32) -> AuthSession {
        AuthSession::Cookie(SessionEntity {
            id,
            secret: SessionSecret::random(),
            user_id: 1,
            user_pubkey: Keypair::random().public_key(),
            capabilities: Capabilities::default(),
            created_at: chrono::Utc::now().naive_utc(),
        })
    }

    fn grant_session(id: GrantId) -> AuthSession {
        AuthSession::Grant(GrantSession {
            user_key: Keypair::random().public_key(),
            capabilities: Capabilities::default(),
            grant_id: id,
            token_expires_at: u64::MAX,
        })
    }

    #[test]
    fn revocations_match_only_their_own_authentication_method() {
        let grant_id = GrantId::generate();
        assert!(AuthRevocation::CookieSession(7).matches(&cookie_session(7)));
        assert!(!AuthRevocation::CookieSession(8).matches(&cookie_session(7)));
        assert!(AuthRevocation::Grant(grant_id.clone()).matches(&grant_session(grant_id)));
        assert!(!AuthRevocation::CookieSession(7).matches(&grant_session(GrantId::generate())));
        assert!(AuthRevocation::All.matches(&cookie_session(7)));
    }

    #[test]
    fn wire_payload_has_no_all_streams_variant() {
        let json = serde_json::to_string(&WireAuthRevocation::Grant(GrantId::generate())).unwrap();
        assert!(json.contains("grant"));
        assert!(!json.contains("all"));
    }

    async fn receive_revocation(
        receiver: &mut broadcast::Receiver<AuthRevocation>,
    ) -> AuthRevocation {
        timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("timed out waiting for auth revocation")
            .expect("auth revocation channel unexpectedly closed")
    }

    async fn listener_backend_pid(pool: &PgPool) -> i32 {
        timeout(Duration::from_secs(5), async {
            loop {
                let pid = sqlx::query_scalar::<_, i32>(
                    "SELECT pid FROM pg_stat_activity \
                     WHERE datname = current_database() \
                       AND pid <> pg_backend_pid() \
                       AND query = $1",
                )
                .bind(format!("LISTEN \"{PG_AUTH_REVOCATION_CHANNEL}\""))
                .fetch_optional(pool)
                .await
                .expect("query listener backend");

                if let Some(pid) = pid {
                    return pid;
                }

                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("timed out waiting for listener backend")
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn listeners_on_two_instances_receive_the_same_revocation() {
        let db = SqlDb::test().await;
        let service_a = AuthRevocationService::new();
        let _listener_a = PgAuthRevocationListener::start(db.pool(), service_a.clone())
            .await
            .unwrap();
        let service_b = AuthRevocationService::new();
        let _listener_b = PgAuthRevocationListener::start(db.pool(), service_b.clone())
            .await
            .unwrap();
        let mut receiver_a = service_a.subscribe().unwrap();
        let mut receiver_b = service_b.subscribe().unwrap();

        let payload = serde_json::to_string(&WireAuthRevocation::CookieSession(42)).unwrap();
        sqlx::query("SELECT pg_notify($1, $2)")
            .bind(PG_AUTH_REVOCATION_CHANNEL)
            .bind(payload)
            .execute(db.pool())
            .await
            .unwrap();

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
    async fn listener_disconnect_closes_all_private_streams_before_reconnect() {
        let db = SqlDb::test().await;
        let service = AuthRevocationService::new();
        let _listener = PgAuthRevocationListener::start(db.pool(), service.clone())
            .await
            .unwrap();
        let mut receiver = service.subscribe().unwrap();

        let listener_pid = listener_backend_pid(db.pool()).await;
        let terminated: bool = sqlx::query_scalar("SELECT pg_terminate_backend($1)")
            .bind(listener_pid)
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert!(
            terminated,
            "Postgres should terminate the listener connection"
        );

        assert_eq!(receive_revocation(&mut receiver).await, AuthRevocation::All);

        // The listener reconnects asynchronously; accepting a new subscription
        // only after that succeeds keeps the failure window fail-closed.
        timeout(Duration::from_secs(5), async {
            loop {
                if service.subscribe().is_ok() {
                    return;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("listener should reconnect");
    }
}
