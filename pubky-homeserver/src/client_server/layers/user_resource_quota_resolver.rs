//! Middleware that resolves per-user limits and inserts them into request extensions.
//!
//! Looks up the user in the DB (with a shared cache) and inserts their
//! `UserResourceQuota` into request extensions. If the user does not exist in the
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

use pubky_common::crypto::PublicKey;

use crate::client_server::extractors::PubkyHost;
use crate::data_directory::user_resource_quota::{
    CachedUserResourceQuota, UserResourceQuota, UserResourceQuotaCache,
    MAX_CACHED_USER_RESOURCE_QUOTAS,
};
use crate::persistence::sql::user::UserRepository;
use crate::persistence::sql::SqlDb;

/// How often the background task runs to evict expired cache entries.
const CLEANUP_INTERVAL_SECS: u64 = 60;

#[derive(Debug, Clone)]
pub struct UserResourceQuotaResolverLayer {
    cache: UserResourceQuotaCache,
    sql_db: SqlDb,
}

impl UserResourceQuotaResolverLayer {
    pub fn new(cache: UserResourceQuotaCache, sql_db: SqlDb) -> Self {
        // Spawn a periodic cleanup task to evict expired entries and prevent
        // unbounded memory growth from requests with rotating public keys.
        let cache_weak = Arc::downgrade(&cache);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
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

impl<S> Layer<S> for UserResourceQuotaResolverLayer {
    type Service = UserResourceQuotaResolverMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        UserResourceQuotaResolverMiddleware {
            inner,
            cache: self.cache.clone(),
            sql_db: self.sql_db.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserResourceQuotaResolverMiddleware<S> {
    inner: S,
    cache: UserResourceQuotaCache,
    sql_db: SqlDb,
}

/// Resolve limits for a single user: check cache, fall back to DB on miss.
///
/// Returns `Some(config)` for known users, `None` for unknown or on error.
async fn resolve_limits(
    pubkey: &PublicKey,
    cache: &UserResourceQuotaCache,
    sql_db: &SqlDb,
) -> Option<UserResourceQuota> {
    // Check cache: use entry if present and not expired.
    // `Some(Some(config))` = cached positive hit
    // `Some(None)` = cached negative hit (user not found)
    // `None` = cache miss or expired
    let cached = cache
        .get(pubkey)
        .filter(|entry| !entry.is_expired())
        .map(|entry| entry.config.clone());

    if let Some(maybe_config) = cached {
        return maybe_config;
    }

    // Cache miss or expired — query DB.
    cache.remove(pubkey);

    // Evict expired entries if at capacity to prevent unbounded growth.
    if cache.len() >= MAX_CACHED_USER_RESOURCE_QUOTAS {
        cache.retain(|_, entry| !entry.is_expired());
    }

    match UserRepository::get(pubkey, &mut sql_db.pool().into()).await {
        Ok(user) => {
            let resolved = user.resource_quota();
            cache.insert(
                pubkey.clone(),
                CachedUserResourceQuota::new(resolved.clone()),
            );
            Some(resolved)
        }
        // Cache a negative entry with a short TTL to prevent repeated DB queries
        // for non-existent users, while allowing subsequent signup to take effect.
        Err(sqlx::Error::RowNotFound) => {
            cache.insert(pubkey.clone(), CachedUserResourceQuota::not_found());
            None
        }
        Err(e) => {
            tracing::warn!(
                "Failed to query user limits for {}: {e}; skipping",
                pubkey.z32()
            );
            None
        }
    }
}

impl<S> Service<Request<Body>> for UserResourceQuotaResolverMiddleware<S>
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
            if let Some(pubky_host) = req.extensions().get::<PubkyHost>().cloned() {
                let pubkey = pubky_host.public_key().clone();
                if let Some(config) = resolve_limits(&pubkey, &cache, &sql_db).await {
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
    use crate::data_directory::user_resource_quota::UserResourceQuota;
    use crate::persistence::sql::user::UserRepository;
    use crate::persistence::sql::SqlDb;

    use super::*;

    /// Handler that extracts the resolved limits and returns them as JSON.
    async fn echo_limits(limits: Option<Extension<UserResourceQuota>>) -> impl IntoResponse {
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

    fn build_test_app(cache: UserResourceQuotaCache, sql_db: SqlDb) -> Router {
        Router::new()
            .route("/test", get(echo_limits))
            .layer(UserResourceQuotaResolverLayer::new(cache, sql_db))
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_no_pubky_host_skips() {
        let db = SqlDb::test().await;
        let cache: UserResourceQuotaCache = Arc::new(dashmap::DashMap::new());
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
        let cache: UserResourceQuotaCache = Arc::new(dashmap::DashMap::new());
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

        // Unknown user should be negatively cached (short TTL) to prevent DB amplification
        assert!(cache.contains_key(&pubkey));
        let entry = cache.get(&pubkey).unwrap();
        assert!(
            entry.config.is_none(),
            "negative cache entry should have no config"
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_resolver_user_with_resource_quota() {
        let db = SqlDb::test().await;
        let cache: UserResourceQuotaCache = Arc::new(dashmap::DashMap::new());

        let pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let custom = UserResourceQuota {
            storage_quota_mb: Some(999),
            max_sessions: None, // unlimited
            ..Default::default()
        };
        UserRepository::set_resource_quota(user.id, &custom, &mut db.pool().into())
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
        let cache: UserResourceQuotaCache = Arc::new(dashmap::DashMap::new());

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
        let cache: UserResourceQuotaCache = Arc::new(dashmap::DashMap::new());

        let pubkey = Keypair::random().public_key();
        // Pre-populate cache with custom values (no user in DB needed for cache hit)
        let cached_config = UserResourceQuota {
            storage_quota_mb: Some(777),
            max_sessions: Some(3),
            ..Default::default()
        };
        cache.insert(pubkey.clone(), CachedUserResourceQuota::new(cached_config));

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
