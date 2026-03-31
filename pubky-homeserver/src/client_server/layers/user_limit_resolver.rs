//! Middleware that resolves per-user limits and inserts them into request extensions.
//!
//! Looks up the user in the DB (with a shared cache) and inserts their
//! `UserLimitConfig` into request extensions. If the user does not exist in the
//! DB, no config is inserted — downstream layers simply see no extension and
//! skip enforcement.
//!
//! The cache is shared with the admin server for immediate eviction on PUT/DELETE.

use std::convert::Infallible;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use futures_util::future::BoxFuture;
use tower::{Layer, Service};

use crate::client_server::extractors::PubkyHost;
use crate::data_directory::user_limit_config::{
    CachedUserLimits, UserLimitsCache, MAX_CACHED_USER_LIMITS,
};
use crate::persistence::sql::user::UserRepository;
use crate::persistence::sql::SqlDb;

#[derive(Debug, Clone)]
pub struct UserLimitResolverLayer {
    cache: UserLimitsCache,
    sql_db: SqlDb,
}

impl UserLimitResolverLayer {
    pub fn new(cache: UserLimitsCache, sql_db: SqlDb) -> Self {
        // Spawn a periodic cleanup task to evict expired entries and prevent
        // unbounded memory growth from requests with rotating public keys.
        let cache_weak = Arc::downgrade(&cache);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                let Some(cache) = cache_weak.upgrade() else {
                    break;
                };
                cache.retain(|_, entry| !entry.is_expired());
            }
        });

        Self { cache, sql_db }
    }
}

impl<S> Layer<S> for UserLimitResolverLayer {
    type Service = UserLimitResolverMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UserLimitResolverMiddleware {
            inner,
            cache: self.cache.clone(),
            sql_db: self.sql_db.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserLimitResolverMiddleware<S> {
    inner: S,
    cache: UserLimitsCache,
    sql_db: SqlDb,
}

impl<S> Service<Request<Body>> for UserLimitResolverMiddleware<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Send
        + 'static
        + Clone,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();
        let cache = self.cache.clone();
        let sql_db = self.sql_db.clone();

        Box::pin(async move {
            // Only resolve if we have a PubkyHost
            if let Some(pubky_host) = req.extensions().get::<PubkyHost>().cloned() {
                let pubkey = pubky_host.public_key().clone();

                // Check cache: use entry if present and not expired.
                let cached_hit = cache
                    .get(&pubkey)
                    .filter(|entry| !entry.is_expired())
                    .map(|entry| entry.config.clone());

                let resolved = if let Some(config) = cached_hit {
                    Some(config)
                } else {
                    // Cache miss or expired — query DB for user entity.
                    // Remove stale entry if present.
                    cache.remove(&pubkey);

                    match UserRepository::get(&pubkey, &mut sql_db.pool().into()).await {
                        Ok(user) => {
                            // The DB is the source of truth: every user row has explicit
                            // limits (set during signup or backfilled by migration).
                            // All-NULL columns = all unlimited.
                            let resolved = user.limits();
                            // Evict expired entries if at capacity to prevent unbounded growth.
                            if cache.len() >= MAX_CACHED_USER_LIMITS {
                                cache.retain(|_, entry| !entry.is_expired());
                            }
                            cache.insert(pubkey, CachedUserLimits::new(resolved.clone()));
                            Some(resolved)
                        }
                        // User not found — no limits to insert. Downstream layers
                        // will see no extension and skip enforcement.
                        // Not cached so a subsequent signup can populate it.
                        Err(sqlx::Error::RowNotFound) => None,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to query user limits for {}: {e}; skipping",
                                pubkey.z32()
                            );
                            None
                        }
                    }
                };

                if let Some(config) = resolved {
                    req.extensions_mut().insert(config);
                }
            }

            inner.call(req).await
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::{Extension, Router};
    use pubky_common::crypto::Keypair;
    use tower::ServiceExt;

    use crate::client_server::extractors::PubkyHost;
    use crate::data_directory::user_limit_config::UserLimitConfig;
    use crate::persistence::sql::user::UserRepository;
    use crate::persistence::sql::SqlDb;

    use super::*;

    /// Handler that extracts the resolved limits and returns them as JSON.
    async fn echo_limits(limits: Option<Extension<UserLimitConfig>>) -> impl IntoResponse {
        match limits {
            Some(Extension(config)) => {
                let body = serde_json::json!({
                    "storage_quota_mb": config.storage_quota_mb,
                    "max_sessions": config.max_sessions,
                });
                (StatusCode::OK, body.to_string())
            }
            None => (StatusCode::OK, "no_limits".to_string()),
        }
    }

    fn build_test_app(cache: UserLimitsCache, sql_db: SqlDb) -> Router {
        Router::new()
            .route("/test", get(echo_limits))
            .layer(UserLimitResolverLayer::new(cache, sql_db))
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_no_pubky_host_skips() {
        let db = SqlDb::test().await;
        let cache: UserLimitsCache = Arc::new(dashmap::DashMap::new());
        let app = build_test_app(cache, db);

        let req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(body, "no_limits");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_unknown_user_inserts_no_limits() {
        let db = SqlDb::test().await;
        let cache: UserLimitsCache = Arc::new(dashmap::DashMap::new());
        let app = build_test_app(cache.clone(), db);

        let pubkey = Keypair::random().public_key();
        let mut req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        // No user in DB → no limits extension inserted
        assert_eq!(body, "no_limits");

        // Unknown user should NOT be cached
        assert!(!cache.contains_key(&pubkey));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_user_with_custom_limits() {
        let db = SqlDb::test().await;
        let cache: UserLimitsCache = Arc::new(dashmap::DashMap::new());

        let pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let custom = UserLimitConfig {
            storage_quota_mb: Some(999),
            max_sessions: None, // unlimited
            ..Default::default()
        };
        UserRepository::set_custom_limits(user.id, &custom, &mut db.pool().into())
            .await
            .unwrap();

        let app = build_test_app(cache.clone(), db);

        let mut req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Custom limits used — not defaults
        assert_eq!(json["storage_quota_mb"], 999);
        assert!(json["max_sessions"].is_null()); // unlimited, not the default 5

        // Should now be cached
        assert!(cache.contains_key(&pubkey));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_user_with_null_columns_returns_unlimited() {
        let db = SqlDb::test().await;
        let cache: UserLimitsCache = Arc::new(dashmap::DashMap::new());

        // Create a user but do NOT set any custom limits — all columns remain NULL.
        let pubkey = Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let app = build_test_app(cache.clone(), db);

        let mut req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // All-NULL columns → all unlimited (null), NOT the deploy-time defaults
        assert!(json["storage_quota_mb"].is_null());
        assert!(json["max_sessions"].is_null());

        // Should be cached (user exists in DB)
        assert!(cache.contains_key(&pubkey));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_cache_hit() {
        let db = SqlDb::test().await;
        let cache: UserLimitsCache = Arc::new(dashmap::DashMap::new());

        let pubkey = Keypair::random().public_key();
        // Pre-populate cache with custom values (no user in DB needed for cache hit)
        let cached_config = UserLimitConfig {
            storage_quota_mb: Some(777),
            max_sessions: Some(3),
            ..Default::default()
        };
        cache.insert(pubkey.clone(), CachedUserLimits::new(cached_config));

        let app = build_test_app(cache, db);

        let mut req = axum::http::Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(PubkyHost(pubkey.clone()));

        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Should return cached values, not defaults
        assert_eq!(json["storage_quota_mb"], 777);
        assert_eq!(json["max_sessions"], 3);
    }
}
