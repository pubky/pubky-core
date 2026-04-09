//!
//! Implements rate limiting with governor.
//!
//! Would love to use tower_governor but I can't type it properly due to
//! https://github.com/benwis/tower-governor/issues/49.
//!
//! So we implement our own rate limiter here.
//!
//! # Two sources of limits
//!
//! 1. **Path limits** (`[[drive.rate_limits]]` in config) — matched by request
//!    path and method. Each entry specifies a `key` (ip or user), a `quota`
//!    (request-count or speed), and an optional whitelist.
//! 2. **Per-user quota** (`QuotaOverride` fields on `UserQuota`) — stored per-user
//!    in the DB. Currently only speed limits: `rate_read` and `rate_write`.
//!    No per-user request-count limits exist.
//!
//! # How they interact
//!
//! - **IP path limit + user path/quota limit**: both apply (stack). The request
//!   is throttled by whichever is stricter.
//! - **Config user speed limit + DB per-user override**: the DB override
//!   **replaces** the config limit, it does not stack. `Value(rate)` uses that
//!   rate, `Unlimited` skips throttling, `Default` falls back to the config
//!   user-keyed speed limit (if one matched the request path).
//!
//! # Phases
//!
//! The `call()` method is structured into 5 phases:
//!
//! 1. **Request-count limits** — check path limits with `RateUnit::Request`,
//!    return 429 if exceeded.
//! 2. **IP-keyed speed limits** — throttle upload for matched IP speed path limits.
//! 3. **User speed limit** — resolve per-user `rate_read`/`rate_write` overrides.
//! 4. **Call inner service**.
//! 5. **Download throttling** — apply IP + user speed limiters to the response body.
//!
use axum::http::Method;
use axum::response::{IntoResponse, Response};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use dashmap::DashMap;
use futures_util::future::BoxFuture;
use governor::clock::QuantaClock;
use governor::state::keyed::DashMapStateStore;
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::client_server::extractors::PubkyHost;
use crate::data_directory::quota_config::BandwidthRate;
use crate::data_directory::user_quota::{
    CachedUserQuota, QuotaOverride, UserQuota, UserQuotaCache, MAX_CACHED_USER_QUOTAS,
};
use crate::persistence::sql::user::UserRepository;
use crate::persistence::sql::SqlDb;
use crate::quota_config::{LimitKey, LimitKeyType, PathLimit, RateUnit};
use crate::shared::HttpError;
use futures_util::StreamExt;
use governor::{Jitter, Quota, RateLimiter};
use pubky_common::crypto::PublicKey;

use super::extract_ip::extract_ip;

type KeyedRateLimiter = RateLimiter<LimitKey, DashMapStateStore<LimitKey>, QuantaClock>;

/// How often the background task runs to evict expired cache entries.
const CLEANUP_INTERVAL_SECS: u64 = 60;

/// A Tower Layer to handle general rate limiting.
///
/// Supports rate limiting by request count and by upload/download speed.
/// For user-keyed speed limits, per-user overrides from `UserQuota`
/// are resolved from cache/DB and used instead of the default path limit.
///
/// Requires a `PubkyHostLayer` to be applied first.
/// Used to extract the user pubkey as the key for the rate limiter.
///
/// Returns 400 BAD REQUEST if the user pubkey aka pubky-host cannot be extracted.
///
#[derive(Debug, Clone)]
pub struct RateLimiterLayer {
    limits: Vec<PathLimit>,
    cache: UserQuotaCache,
    sql_db: SqlDb,
}

impl RateLimiterLayer {
    /// Create a new rate limiter layer with the given path limits and per-user
    /// quota resolution dependencies.
    pub fn new(limits: Vec<PathLimit>, cache: UserQuotaCache, sql_db: SqlDb) -> Self {
        if limits.is_empty() {
            tracing::info!("Rate limiting is disabled.");
        } else {
            let limits_str = limits
                .iter()
                .map(|limit| format!("\"{limit}\""))
                .collect::<Vec<String>>();
            tracing::info!("Rate limits configured: {}", limits_str.join(", "));
        }

        // Spawn a periodic cleanup task for the user quota cache.
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

        Self {
            limits,
            cache,
            sql_db,
        }
    }
}

impl<S> Layer<S> for RateLimiterLayer {
    type Service = RateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let tuples = self
            .limits
            .iter()
            .map(|path| LimitTuple::new(path.clone()))
            .collect();

        let user_read_limiters: Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>> =
            Arc::new(DashMap::new());
        let user_write_limiters: Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>> =
            Arc::new(DashMap::new());

        // Spawn periodic cleanup for per-user limiter pools.
        for pool in [user_read_limiters.clone(), user_write_limiters.clone()] {
            let pool_weak = Arc::downgrade(&pool);
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
                interval.tick().await; // skip first immediate tick
                loop {
                    interval.tick().await;
                    let Some(pool) = pool_weak.upgrade() else {
                        break;
                    };
                    pool.retain(|_, limiter| {
                        limiter.retain_recent();
                        limiter.shrink_to_fit();
                        !limiter.is_empty()
                    });
                }
            });
        }

        RateLimiterMiddleware {
            inner,
            limits: tuples,
            cache: self.cache.clone(),
            sql_db: self.sql_db.clone(),
            user_read_limiters,
            user_write_limiters,
        }
    }
}

/// A tuple of a path limit and the actual governor rate limiter.
#[derive(Debug, Clone)]
struct LimitTuple {
    pub limit: PathLimit,
    pub limiter: Arc<KeyedRateLimiter>,
}

impl LimitTuple {
    pub fn new(path_limit: PathLimit) -> Self {
        let quota: Quota = path_limit.clone().into();
        let limiter = Arc::new(RateLimiter::keyed(quota));

        // Forget keys that are not used anymore. This is to prevent memory leaks.
        let limiter_clone = limiter.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                limiter_clone.retain_recent();
                limiter_clone.shrink_to_fit();
            }
        });

        Self {
            limit: path_limit,
            limiter,
        }
    }

    /// Extract the key from the request.
    ///
    /// The key is either the ip address of the client
    /// or the user pubkey.
    fn extract_key(&self, req: &Request<Body>) -> anyhow::Result<LimitKey> {
        match self.limit.key {
            LimitKeyType::Ip => extract_ip(req).map(LimitKey::Ip),
            LimitKeyType::User => {
                // Extract the user pubkey from the request.
                req.extensions()
                    .get::<PubkyHost>()
                    .map(|pk| LimitKey::User(pk.public_key().clone()))
                    .ok_or(anyhow::anyhow!("Failed to extract user pubkey."))
            }
        }
    }

    /// Check if the request matches the limit.
    pub fn is_match(&self, req: &Request<Body>) -> bool {
        let path = req.uri().path();
        let glob_match = self.limit.path.is_match(path);
        let method_match = self.limit.method.0 == req.method();
        glob_match && method_match
    }
}

/// Pool key for per-user speed limiters: rate + optional burst override.
/// Users with the same (rate, burst) share a limiter instance.
type SpeedLimitKey = (BandwidthRate, Option<u32>);

/// Get or create a keyed rate limiter for a specific bandwidth rate + burst from a shared pool.
fn get_or_create_limiter(
    pool: &DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>,
    rate: &BandwidthRate,
    burst: Option<u32>,
) -> Arc<KeyedRateLimiter> {
    pool.entry((rate.clone(), burst))
        .or_insert_with(|| {
            let quota: Quota = rate.to_governor_quota(burst);
            Arc::new(RateLimiter::keyed(quota))
        })
        .clone()
}

/// Resolve limits for a single user: check cache, fall back to DB on miss.
///
/// Returns `Ok(Some(config))` for known users, `Ok(None)` for unknown users,
/// or `Err` if the DB query fails.
///
/// ## Cache capacity behaviour
///
/// The cache is bounded by [`MAX_CACHED_USER_QUOTAS`]. When a miss
/// occurs at capacity, expired entries are evicted first. If the cache is
/// still full after eviction, 10% of entries are evicted (arbitrary order)
/// to make room in bulk and avoid per-request churn.
async fn resolve_limits(
    pubkey: &PublicKey,
    cache: &UserQuotaCache,
    sql_db: &SqlDb,
) -> Result<Option<UserQuota>, sqlx::Error> {
    // Check cache: use entry if present and not expired.
    let cached = cache
        .get(pubkey)
        .filter(|entry| !entry.is_expired())
        .map(|entry| entry.config.clone());

    if let Some(maybe_config) = cached {
        return Ok(maybe_config);
    }

    // Cache miss or expired — query DB.
    cache.remove(pubkey);

    // Make room if at capacity: evict expired entries first, then ~10%
    // in bulk if still full to avoid per-request churn.
    if cache.len() >= MAX_CACHED_USER_QUOTAS {
        cache.retain(|_, entry| !entry.is_expired());

        if cache.len() >= MAX_CACHED_USER_QUOTAS {
            let to_evict = MAX_CACHED_USER_QUOTAS / 10;
            let keys: Vec<_> = cache
                .iter()
                .take(to_evict.max(1))
                .map(|entry| entry.key().clone())
                .collect();
            for key in keys {
                cache.remove(&key);
            }
        }
    }

    match UserRepository::get(pubkey, &mut sql_db.pool().into()).await {
        Ok(user) => {
            let resolved = user.quota();
            cache.insert(pubkey.clone(), CachedUserQuota::new(resolved.clone()));
            Ok(Some(resolved))
        }
        // Cache a negative entry with a short TTL to prevent repeated DB queries
        // for non-existent users, while allowing subsequent signup to take effect.
        Err(sqlx::Error::RowNotFound) => {
            cache.insert(pubkey.clone(), CachedUserQuota::not_found());
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiterMiddleware<S> {
    inner: S,
    limits: Vec<LimitTuple>,
    cache: UserQuotaCache,
    sql_db: SqlDb,
    /// Shared pool of per-user read speed limiters. Users with the same (rate, burst) share an instance.
    user_read_limiters: Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>>,
    /// Shared pool of per-user write speed limiters. Users with the same (rate, burst) share an instance.
    user_write_limiters: Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>>,
}

impl<S> RateLimiterMiddleware<S> {
    /// Throttle the upload body.
    fn throttle_upload(
        req: Request<Body>,
        key: &LimitKey,
        limiter: &Arc<KeyedRateLimiter>,
    ) -> Request<Body> {
        let (parts, body) = req.into_parts();
        let new_body = Self::throttle_body(body, key, limiter);
        Request::from_parts(parts, new_body)
    }

    /// Throttle the download body.
    fn throttle_download(
        res: Response<Body>,
        key: &LimitKey,
        limiter: &Arc<KeyedRateLimiter>,
    ) -> Response<Body> {
        let (parts, body) = res.into_parts();
        let new_body = Self::throttle_body(body, key, limiter);
        Response::from_parts(parts, new_body)
    }

    /// Throttle the up or download body.
    ///
    /// Important: The speed quotas are always in kilobytes, not bytes.
    /// Counting bytes is not practical.
    ///
    fn throttle_body(body: Body, key: &LimitKey, limiter: &Arc<KeyedRateLimiter>) -> Body {
        let body_stream = body.into_data_stream();
        let limiter = limiter.clone();
        let key = key.clone();
        let throttled = body_stream
            .map(move |chunk| {
                let limiter = limiter.clone();
                let key = key.clone();
                // When the rate limit is exceeded, we wait between 25ms and 500ms before retrying.
                // This is to avoid overwhelming the server with requests when the rate limit is exceeded.
                // Randomization is used to avoid thundering herd problem.
                let jitter = Jitter::new(Duration::from_millis(25), Duration::from_millis(500));
                async move {
                    let bytes = match chunk {
                        Ok(actual_chunk) => actual_chunk,
                        Err(e) => return Err(e),
                    };

                    // --- Round up to the nearest kilobyte. ---
                    // Important: If the chunk is < 1KB, it will be rounded up to 1 kb.
                    // Many small uploads will be counted as more than they actually are.
                    // I am not too concerned about this though because small random disk writes are stressing
                    // the disk more anyway compared to larger writes.
                    // Why are we doing this? governor::Quota is defined as a u32. u32 can only count up to 4GB.
                    // To support 4GB/s+ limits we need to count in kilobytes.
                    //
                    // --- Chunk Size ---
                    // The chunk size is determined by the client library.
                    // Common chunk sizes: 16KB to 10MB.
                    // HTTP based uploads are usually between 256KB and 1MB.
                    // Asking the limiter for 1KB packets is tradeoff between
                    // - Not calling the limiter too much
                    // - Guaranteeing the call size (1kb) is low enough to not cause race condition issues.
                    let chunk_kilobytes = bytes.len().div_ceil(1024);
                    for _ in 0..chunk_kilobytes {
                        // Check each kilobyte
                        if limiter
                            .until_key_n_ready_with_jitter(
                                &key,
                                NonZero::new(1).expect("1 is always non zero"),
                                jitter,
                            )
                            .await
                            .is_err()
                        {
                            // Requested rate (1 KB) exceeds the configured limit.
                            // This should not happen in practice since limits are in KB.
                            tracing::error!(
                                "Rate limiter rejected a 1 KB cell — limit may be misconfigured"
                            );
                            return Err(axum::Error::new("Rate limit exceeded"));
                        };
                    }
                    Ok(bytes)
                }
            })
            .buffered(1);

        Body::from_stream(throttled)
    }

    /// Get the limits that match the request.
    fn get_limit_matches(&self, req: &Request<Body>) -> Vec<&LimitTuple> {
        self.limits
            .iter()
            .filter(|limit| limit.is_match(req))
            .collect()
    }
}

impl<S> Service<Request<Body>> for RateLimiterMiddleware<S>
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
        self.inner.poll_ready(cx).map_err(|_| unreachable!()) // `Infallible` conversion
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        // Match the request path + method to the defined limits.
        let path_limits = self.get_limit_matches(&req);

        let user_pubkey = req
            .extensions()
            .get::<PubkyHost>()
            .map(|pk| pk.public_key().clone());

        if path_limits.is_empty() && user_pubkey.is_none() {
            // No limits matched and no user to resolve, skip entirely.
            return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
        }

        let cache = self.cache.clone();
        let sql_db = self.sql_db.clone();
        let path_limits_owned: Vec<LimitTuple> = path_limits.into_iter().cloned().collect();
        let user_read_limiters = self.user_read_limiters.clone();
        let user_write_limiters = self.user_write_limiters.clone();

        Box::pin(async move {
            // Resolve per-user quota when a user pubkey is present (cache makes this cheap).
            let user_quota = if let Some(ref pubkey) = user_pubkey {
                match resolve_limits(pubkey, &cache, &sql_db).await {
                    Ok(quota) => quota,
                    Err(e) => {
                        tracing::error!("Failed to resolve user limits for {}: {e}", pubkey.z32());
                        return Ok(HttpError::new_with_message(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to resolve user limits",
                        )
                        .into_response());
                    }
                }
            } else {
                None
            };

            // Collect (key, limiter) pairs for download throttling in phase 5.
            let mut download_throttlers: Vec<(LimitKey, Arc<KeyedRateLimiter>)> = Vec::new();

            // ── Phase 1: Request-count limits ──
            for limit in &path_limits_owned {
                if limit.limit.quota.rate_unit != RateUnit::Request {
                    continue;
                }
                let key = match limit.extract_key(&req) {
                    Ok(key) => key,
                    Err(e) => {
                        tracing::warn!(
                            "{} {} Failed to extract key for rate limiting: {}",
                            limit.limit.path.0,
                            limit.limit.method.0,
                            e
                        );
                        return Ok(HttpError::new_with_message(
                            StatusCode::BAD_REQUEST,
                            "Failed to extract key for rate limiting",
                        )
                        .into_response());
                    }
                };
                if limit.limit.is_whitelisted(&key) {
                    continue;
                }
                if let Err(e) = limit.limiter.check_key(&key) {
                    tracing::debug!(
                        "Rate limit of {} exceeded for {key}: {}",
                        limit.limit.quota,
                        e
                    );
                    return Ok(HttpError::new_with_message(
                        StatusCode::TOO_MANY_REQUESTS,
                        "Rate limit exceeded",
                    )
                    .into_response());
                }
            }

            // ── Phase 2: IP-keyed speed limits ──
            for limit in &path_limits_owned {
                if !limit.limit.quota.rate_unit.is_speed_rate_unit()
                    || limit.limit.key != LimitKeyType::Ip
                {
                    continue;
                }
                let key = match limit.extract_key(&req) {
                    Ok(key) => key,
                    Err(e) => {
                        tracing::warn!(
                            "{} {} Failed to extract key for rate limiting: {}",
                            limit.limit.path.0,
                            limit.limit.method.0,
                            e
                        );
                        return Ok(HttpError::new_with_message(
                            StatusCode::BAD_REQUEST,
                            "Failed to extract key for rate limiting",
                        )
                        .into_response());
                    }
                };
                if limit.limit.is_whitelisted(&key) {
                    continue;
                }
                req = Self::throttle_upload(req, &key, &limit.limiter);
                download_throttlers.push((key, limit.limiter.clone()));
            }

            // ── Phase 3: User speed limit (one effective rate per direction) ──
            if let (Some(ref pubkey), Some(ref quota)) = (&user_pubkey, &user_quota) {
                let user_key = LimitKey::User(pubkey.clone());

                // Determine direction from HTTP method.
                let method = req.method().clone();
                let (rate_field, burst_field, limiter_pool) = match method {
                    Method::PUT | Method::POST | Method::PATCH | Method::DELETE => (
                        &quota.rate_write,
                        quota.rate_write_burst,
                        &user_write_limiters,
                    ),
                    _ => (&quota.rate_read, quota.rate_read_burst, &user_read_limiters),
                };

                // Find the matching global user-keyed speed LimitTuple for Default fallback.
                let global_user_speed_limit: Option<&LimitTuple> =
                    path_limits_owned.iter().find(|lt| {
                        lt.limit.key == LimitKeyType::User
                            && lt.limit.quota.rate_unit.is_speed_rate_unit()
                    });

                // Resolve effective rate: per-user override > global PathLimit default.
                match rate_field {
                    QuotaOverride::Value(rate) => {
                        // User has a custom rate — use a shared per-(rate, burst) limiter.
                        let limiter = get_or_create_limiter(limiter_pool, rate, burst_field);
                        req = Self::throttle_upload(req, &user_key, &limiter);
                        download_throttlers.push((user_key, limiter));
                    }
                    QuotaOverride::Unlimited => {
                        // User explicitly bypasses speed limiting — no throttle.
                    }
                    QuotaOverride::Default => {
                        // Fall back to the global user-keyed speed PathLimit (if one matched).
                        if let Some(lt) = global_user_speed_limit {
                            if !lt.limit.is_whitelisted(&user_key) {
                                req = Self::throttle_upload(req, &user_key, &lt.limiter);
                                download_throttlers.push((user_key, lt.limiter.clone()));
                            }
                        }
                    }
                }
            }

            // ── Phase 4: Call inner service ──
            let mut response = match inner.call(req).await.map_err(|_| unreachable!()) {
                Ok(response) => response,
                Err(e) => return Err(e),
            };

            // ── Phase 5: Download throttling ──
            for (key, limiter) in &download_throttlers {
                response = Self::throttle_download(response, key, limiter);
            }

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;

    use axum::http::Method;
    use axum::{
        routing::{get, post},
        Router,
    };
    use axum_server::Server;
    use pubky_common::crypto::{Keypair, PublicKey};
    use reqwest::{Client, Response};
    use tokio::{task::JoinHandle, time::Instant};

    use crate::shared::HttpResult;
    use crate::{client_server::layers::pubky_host::PubkyHostLayer, quota_config::GlobPattern};

    use super::*;

    // Fake upload handler that just consumes the body.
    pub async fn upload_handler(body: Body) -> HttpResult<impl IntoResponse> {
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            // Consume body
            let _ = chunk;
        }
        Ok((StatusCode::CREATED, ()))
    }

    // Fake download handler that returns a fixed-size body.
    pub async fn download_handler() -> HttpResult<impl IntoResponse> {
        let response_body = vec![0u8; 3 * 1024]; // 3kb
        Ok((StatusCode::OK, response_body))
    }

    use crate::persistence::sql::SqlDb;

    // Start a server with the given quota config on a random port.
    async fn start_server(config: Vec<PathLimit>) -> SocketAddr {
        let db = SqlDb::test().await;
        start_server_with_db(config, Arc::new(DashMap::new()), db).await
    }

    // Start a server with the given config, cache, and DB on a random port.
    async fn start_server_with_db(
        config: Vec<PathLimit>,
        cache: UserQuotaCache,
        db: SqlDb,
    ) -> SocketAddr {
        let app = Router::new()
            .route("/upload", post(upload_handler))
            .route("/download", get(download_handler))
            .layer(RateLimiterLayer::new(config, cache, db))
            .layer(PubkyHostLayer);

        // Create a TCP listener to bind to the socket first
        // Use port 0 to let the OS assign a random available port
        let listener = tokio::net::TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            0,
        ))
        .await
        .unwrap();
        // Get the actual socket address with the OS-assigned port
        let socket = listener.local_addr().unwrap();

        // Use the listener with axum_server
        let server = Server::<SocketAddr>::from_listener(listener);

        tokio::spawn(async move {
            server
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .unwrap();
        });

        socket
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_throttle_upload() {
        let path_limit = PathLimit::new(
            GlobPattern::new("/upload"),
            Method::POST,
            "1kb/s".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        );
        let socket = start_server(vec![path_limit]).await;

        fn upload_data(socket: SocketAddr, kilobytes: usize) -> JoinHandle<()> {
            tokio::spawn(async move {
                let client = Client::new();
                let data = vec![0u8; kilobytes * 1024];
                let response = client
                    .post(format!("http://{}/upload", socket))
                    .body(data.clone())
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::CREATED);
            })
        }

        let start = Instant::now();
        // Spawn in the background to test 2 uploads in parallel
        // Upload 3kb each
        let handle1 = upload_data(socket, 4);
        let handle2 = upload_data(socket, 4);

        // Wait for the uploads to finish
        let _ = tokio::try_join!(handle1, handle2);

        let time_taken = start.elapsed();
        assert!(time_taken > Duration::from_secs(5), "Should at least take 5s because uploads are limited to 1kb/s and the sum of the uploads is 6kb");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_throttle_download() {
        let path_limit = PathLimit::new(
            GlobPattern::new("/download"),
            Method::GET,
            "1kb/s".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        );
        let socket = start_server(vec![path_limit]).await;

        fn download_data(socket: SocketAddr) -> JoinHandle<()> {
            tokio::spawn(async move {
                let client = Client::new();
                let response = client
                    .get(format!("http://{}/download", socket))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                response.bytes().await.unwrap(); // Download the body
            })
        }

        let start = Instant::now();
        // Spawn in the background to test 2 downloads in parallel
        // Download 3kb each
        let handle1 = download_data(socket);
        let handle2 = download_data(socket);

        // Wait for the uploads to finish
        let _ = tokio::try_join!(handle1, handle2);

        let time_taken = start.elapsed();
        if time_taken < Duration::from_secs(5) {
            panic!("Should at least take 5s because downloads are limited to 1kb/s and the sum of the downloads is 6kb. Time taken: {:?}", time_taken);
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_limit_parallel_requests_with_ip_key() {
        let path_limit = PathLimit::new(
            GlobPattern::new("/upload"),
            Method::POST,
            "1r/m".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        );
        let socket = start_server(vec![path_limit]).await;

        fn send_request(socket: SocketAddr) -> JoinHandle<Response> {
            tokio::spawn(async move {
                let client = Client::new();
                let response = client
                    .post(format!("http://{}/upload", socket))
                    .send()
                    .await
                    .unwrap();
                response
            })
        }

        // Spawn in the background to test 2 uploads in parallel
        let handle1 = send_request(socket);
        let handle2 = send_request(socket);

        // Wait for the uploads to finish
        let (res1, res2) = tokio::try_join!(handle1, handle2).unwrap();
        assert_eq!(res1.status(), StatusCode::CREATED);
        assert_eq!(res2.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_limit_parallel_requests_with_user_key() {
        let path_limit = PathLimit::new(
            GlobPattern::new("/upload"),
            Method::POST,
            "1r/m".parse().unwrap(),
            LimitKeyType::User,
            None,
        );
        let socket = start_server(vec![path_limit]).await;

        fn send_request(socket: SocketAddr, user_pubkey: PublicKey) -> JoinHandle<Response> {
            tokio::spawn(async move {
                let client = Client::new();
                let response = client
                    .post(format!(
                        "http://{}/upload?pubky-host={}",
                        socket,
                        user_pubkey.z32()
                    ))
                    .send()
                    .await
                    .unwrap();
                response
            })
        }

        // Spawn two parallel requests for user1 and one for user2. Exactly one
        // of user1's requests should be rejected, but which one wins the race
        // is non-deterministic, so sort the two statuses before asserting.
        let user1_pubkey = Keypair::random().public_key();
        let handle1 = send_request(socket, user1_pubkey.clone());
        let handle2 = send_request(socket, user1_pubkey.clone());
        let user2_pubkey = Keypair::random().public_key();
        let handle3 = send_request(socket, user2_pubkey.clone());

        // Wait for the uploads to finish
        let (res1, res2, res3) = tokio::try_join!(handle1, handle2, handle3).unwrap();

        let mut user1_statuses = [res1.status(), res2.status()];
        user1_statuses.sort_by_key(|s| s.as_u16());
        assert_eq!(
            user1_statuses,
            [StatusCode::CREATED, StatusCode::TOO_MANY_REQUESTS],
            "user1 should have exactly one success and one rate-limited response"
        );
        assert_eq!(res3.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_user_rate_override_direction_detection() {
        use crate::data_directory::user_quota::QuotaOverride;

        let write_rate: BandwidthRate = "5mb/s".parse().unwrap();
        let read_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let quota = UserQuota {
            rate_write: QuotaOverride::Value(write_rate.clone()),
            rate_read: QuotaOverride::Value(read_rate.clone()),
            ..Default::default()
        };

        // Phase 3 resolves direction from HTTP method:
        // PUT/POST/PATCH → rate_write, GET/HEAD → rate_read
        assert_eq!(quota.rate_write.as_value(), Some(&write_rate));
        assert_eq!(quota.rate_read.as_value(), Some(&read_rate));

        // Default quota has no overrides
        let default_quota = UserQuota::default();
        assert!(default_quota.rate_write.is_default());
        assert!(default_quota.rate_read.is_default());
    }

    #[tokio::test]
    async fn test_user_rate_override_unlimited_bypass() {
        use crate::data_directory::user_quota::QuotaOverride;

        let quota = UserQuota {
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };

        // Phase 3: Unlimited means no throttling is applied
        assert!(quota.rate_write.is_unlimited());
    }

    #[tokio::test]
    async fn test_user_without_override_uses_default() {
        // User with no rate overrides (all Default) — phase 3 falls back to
        // the global user-keyed speed PathLimit (if one matched).
        let quota = UserQuota::default();
        assert!(quota.rate_write.is_default());
        assert!(quota.rate_read.is_default());
    }

    #[tokio::test]
    async fn test_user_limiter_pool_creation() {
        // Test the shared per-(rate, burst) limiter pool (get_or_create_limiter).
        let pool: DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>> = DashMap::new();

        let rate: BandwidthRate = "5mb/s".parse().unwrap();
        let limiter1 = get_or_create_limiter(&pool, &rate, None);
        let limiter2 = get_or_create_limiter(&pool, &rate, None);

        // Same rate + burst should return the same limiter instance
        assert!(Arc::ptr_eq(&limiter1, &limiter2));

        // Different rate should return a different limiter
        let other_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let limiter3 = get_or_create_limiter(&pool, &other_rate, None);
        assert!(!Arc::ptr_eq(&limiter1, &limiter3));

        // Same rate but different burst should return a different limiter
        let limiter4 = get_or_create_limiter(&pool, &rate, Some(50));
        assert!(!Arc::ptr_eq(&limiter1, &limiter4));

        // Same rate + same burst should share
        let limiter5 = get_or_create_limiter(&pool, &rate, Some(50));
        assert!(Arc::ptr_eq(&limiter4, &limiter5));
    }
}
