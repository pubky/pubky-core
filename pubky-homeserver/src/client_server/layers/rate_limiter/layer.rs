//!
//! Implements rate limiting with governor.
//!
//! Would love to use tower_governor but I can't type it properly due to
//! https://github.com/benwis/tower-governor/issues/49.
//!
//! So we implement our own rate limiter here.
//!
//! Supports optional per-user speed overrides: when a user-keyed speed limit
//! matches and the user has a custom `BandwidthRate` in their `UserResourceQuota`,
//! a separate governor rate limiter is used for that user's speed instead of the
//! path-level default. Users with the same override value share a limiter.
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
use crate::data_directory::user_resource_quota::{
    CachedUserResourceQuota, UserResourceQuota, UserResourceQuotaCache,
    MAX_CACHED_USER_RESOURCE_QUOTAS,
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
/// For user-keyed speed limits, per-user overrides from `UserResourceQuota`
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
    cache: UserResourceQuotaCache,
    sql_db: SqlDb,
}

impl RateLimiterLayer {
    /// Create a new rate limiter layer with the given path limits and per-user
    /// quota resolution dependencies.
    pub fn new(limits: Vec<PathLimit>, cache: UserResourceQuotaCache, sql_db: SqlDb) -> Self {
        if limits.is_empty() {
            tracing::info!("Rate limiting is disabled.");
        } else {
            let limits_str = limits
                .iter()
                .map(|limit| format!("\"{limit}\""))
                .collect::<Vec<String>>();
            tracing::info!("Rate limits configured: {}", limits_str.join(", "));
        }

        // Spawn a periodic cleanup task for the user resource quota cache.
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

        RateLimiterMiddleware {
            inner,
            limits: tuples,
            cache: self.cache.clone(),
            sql_db: self.sql_db.clone(),
        }
    }
}

/// Per-user rate override resolved from `QuotaOverride<BandwidthRate>`.
enum RateOverride<'a> {
    /// User has `Unlimited` — bypass throttling entirely.
    Bypass,
    /// User has `Value(rate)` — use a custom rate limiter.
    Custom(&'a BandwidthRate),
}

/// A tuple of a path limit and the actual governor rate limiter.
#[derive(Debug, Clone)]
struct LimitTuple {
    pub limit: PathLimit,
    pub limiter: Arc<KeyedRateLimiter>,
    /// Per-quota override rate limiters. Users with the same override value
    /// share a limiter. Only used for user-keyed speed limits.
    pub override_limiters: Arc<DashMap<BandwidthRate, Arc<KeyedRateLimiter>>>,
}

impl LimitTuple {
    pub fn new(path_limit: PathLimit) -> Self {
        let quota: Quota = path_limit.clone().into();
        let limiter = Arc::new(RateLimiter::keyed(quota));
        let override_limiters: Arc<DashMap<BandwidthRate, Arc<KeyedRateLimiter>>> =
            Arc::new(DashMap::new());

        // Forget keys that are not used anymore. This is to prevent memory leaks.
        let limiter_clone = limiter.clone();
        let overrides_clone = override_limiters.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await;
            loop {
                interval.tick().await;
                limiter_clone.retain_recent();
                limiter_clone.shrink_to_fit();
                // Clean up override limiters: retain only those with active keys.
                overrides_clone.retain(|_, limiter| {
                    limiter.retain_recent();
                    limiter.shrink_to_fit();
                    !limiter.is_empty()
                });
            }
        });

        Self {
            limit: path_limit,
            limiter,
            override_limiters,
        }
    }

    /// Get or create an override rate limiter for a specific bandwidth rate.
    fn get_or_create_override_limiter(&self, rate: &BandwidthRate) -> Arc<KeyedRateLimiter> {
        self.override_limiters
            .entry(rate.clone())
            .or_insert_with(|| {
                let quota: Quota = rate.clone().into();
                Arc::new(RateLimiter::keyed(quota))
            })
            .clone()
    }

    /// Resolve which rate limiter to use: per-user override, bypass, or default.
    ///
    /// For user-keyed speed limits where the user has a custom `BandwidthRate`,
    /// returns a shared override limiter. If the user's rate is `Unlimited`,
    /// returns `None` to bypass throttling entirely. Otherwise returns the
    /// default limiter.
    fn resolve_effective_limiter(
        &self,
        user_quota: Option<&UserResourceQuota>,
    ) -> Option<Arc<KeyedRateLimiter>> {
        match user_quota.and_then(|q| self.user_rate_override(q)) {
            Some(RateOverride::Bypass) => None,
            Some(RateOverride::Custom(rate)) => Some(self.get_or_create_override_limiter(rate)),
            None => Some(self.limiter.clone()),
        }
    }

    /// Determine the per-user rate override for this limit, if applicable.
    ///
    /// Returns `Some(Bypass)` for Unlimited, `Some(Custom(rate))` for Value,
    /// and `None` for Default or non-applicable limits.
    fn user_rate_override<'a>(&self, quota: &'a UserResourceQuota) -> Option<RateOverride<'a>> {
        use crate::data_directory::user_resource_quota::QuotaOverride;

        // Only applies to user-keyed speed limits
        if !self.limit.quota.rate_unit.is_speed_rate_unit() || self.limit.key != LimitKeyType::User
        {
            return None;
        }
        let field = match self.limit.method.0 {
            Method::PUT | Method::POST | Method::PATCH => &quota.rate_write,
            Method::GET | Method::HEAD => &quota.rate_read,
            _ => return None,
        };
        match field {
            QuotaOverride::Unlimited => Some(RateOverride::Bypass),
            QuotaOverride::Value(rate) => Some(RateOverride::Custom(rate)),
            QuotaOverride::Default => None,
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

/// Resolve limits for a single user: check cache, fall back to DB on miss.
///
/// Returns `Some(config)` for known users, `None` for unknown or on error.
///
/// ## Cache capacity behaviour
///
/// The cache is bounded by [`MAX_CACHED_USER_RESOURCE_QUOTAS`]. When a miss
/// occurs at capacity, expired entries are evicted first. If the cache is
/// still full after eviction, a random entry is evicted to make room.
/// This ensures new entries are always cached, avoiding repeated DB hits
/// for the same user during high-traffic bursts.
async fn resolve_limits(
    pubkey: &PublicKey,
    cache: &UserResourceQuotaCache,
    sql_db: &SqlDb,
) -> Option<UserResourceQuota> {
    // Check cache: use entry if present and not expired.
    let cached = cache
        .get(pubkey)
        .filter(|entry| !entry.is_expired())
        .map(|entry| entry.config.clone());

    if let Some(maybe_config) = cached {
        return maybe_config;
    }

    // Cache miss or expired — query DB.
    cache.remove(pubkey);

    // Make room if at capacity: evict expired entries first, then a random
    // entry if still full.
    if cache.len() >= MAX_CACHED_USER_RESOURCE_QUOTAS {
        cache.retain(|_, entry| !entry.is_expired());

        if cache.len() >= MAX_CACHED_USER_RESOURCE_QUOTAS {
            // Evict a random entry. DashMap iteration order is arbitrary
            // (hash-bucket based), which provides sufficient randomness here.
            if let Some(entry) = cache.iter().next() {
                cache.remove(entry.key());
            }
        }
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

#[derive(Debug, Clone)]
pub struct RateLimiterMiddleware<S> {
    inner: S,
    limits: Vec<LimitTuple>,
    cache: UserResourceQuotaCache,
    sql_db: SqlDb,
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
                let jitter = Jitter::new(
                    Duration::from_millis(25),
                    Duration::from_millis(500),
                );
                async move {
                    let bytes = match chunk {
                        Ok(actual_chunk) => {
                            actual_chunk
                        }
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
                            .await.is_err()
                        {
                            // Requested rate (1kb) is higher then the set limit (x kb/s).
                            // This should never happen.
                            unreachable!("Rate limiting is based on the number of kilobytes, not bytes. So 1 kb should always be allowed.");
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
        let limits = self.get_limit_matches(&req);
        if limits.is_empty() {
            // No limits matched, so we can just call the next layer.
            return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
        }

        // Check if any matched limit is a user-keyed speed limit — if so, we may
        // need to resolve per-user overrides from cache/DB.
        let needs_user_quota = limits.iter().any(|lt| {
            lt.limit.key == LimitKeyType::User && lt.limit.quota.rate_unit.is_speed_rate_unit()
        });

        let user_pubkey = req
            .extensions()
            .get::<PubkyHost>()
            .map(|pk| pk.public_key().clone());

        let cache = self.cache.clone();
        let sql_db = self.sql_db.clone();
        let limits_owned: Vec<LimitTuple> = limits.into_iter().cloned().collect();

        Box::pin(async move {
            // Resolve per-user quota only when a user-keyed speed limit matched.
            let user_quota = if needs_user_quota {
                if let Some(ref pubkey) = user_pubkey {
                    resolve_limits(pubkey, &cache, &sql_db).await
                } else {
                    None
                }
            } else {
                None
            };

            // Collect the resolved limiter for each speed limit (for download throttling later).
            // `None` limiter means the user has Unlimited — bypass throttling.
            let mut speed_limit_entries: Vec<(LimitTuple, Option<Arc<KeyedRateLimiter>>)> =
                Vec::new();

            // Go through all the limits and check if we need to throttle or reject the request.
            for limit in &limits_owned {
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

                match limit.limit.quota.rate_unit {
                    RateUnit::SpeedRateUnit(_) => {
                        let effective_limiter =
                            limit.resolve_effective_limiter(user_quota.as_ref());
                        if let Some(ref limiter) = effective_limiter {
                            req = Self::throttle_upload(req, &key, limiter);
                        }
                        speed_limit_entries.push((limit.clone(), effective_limiter));
                    }
                    RateUnit::Request => {
                        // Request limiting: limit the number of requests.
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
                        };
                    }
                };
            }

            // Create a clone of the request without the body for download throttling.
            let (parts, body) = req.into_parts();
            let req_clone = Request::from_parts(parts.clone(), Body::empty());
            let req = Request::from_parts(parts, body);

            // Call the next layer and receive the response.
            let mut response = match inner.call(req).await.map_err(|_| unreachable!()) {
                Ok(response) => response,
                Err(e) => return Err(e),
            };

            // Rate limit the download speed using the same resolved limiters.
            for (limit, effective_limiter) in &speed_limit_entries {
                if let (Ok(key), Some(limiter)) = (limit.extract_key(&req_clone), effective_limiter)
                {
                    response = Self::throttle_download(response, &key, limiter);
                }
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
        cache: UserResourceQuotaCache,
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

        // Spawn in the background to test 2 uploads in parallel
        let user1_pubkey = Keypair::random().public_key();
        let handle1 = send_request(socket, user1_pubkey.clone());
        let handle2 = send_request(socket, user1_pubkey.clone());
        let user2_pubkey = Keypair::random().public_key();
        let handle3 = send_request(socket, user2_pubkey.clone());

        // Wait for the uploads to finish
        let (res1, res2, res3) = tokio::try_join!(handle1, handle2, handle3).unwrap();
        assert_eq!(res1.status(), StatusCode::CREATED);
        assert_eq!(res2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res3.status(), StatusCode::CREATED);
    }

    /// Helper to create a LimitTuple for `/pub/**` with the given method, quota, and key type.
    fn pub_limit(method: Method, quota: &str, key: LimitKeyType) -> LimitTuple {
        LimitTuple::new(PathLimit::new(
            GlobPattern::new("/pub/**"),
            method,
            quota.parse().unwrap(),
            key,
            None,
        ))
    }

    #[tokio::test]
    async fn test_user_rate_override_direction_detection() {
        use crate::data_directory::user_resource_quota::QuotaOverride;

        let write_limit = pub_limit(Method::PUT, "1mb/s", LimitKeyType::User);
        let read_limit = pub_limit(Method::GET, "1mb/s", LimitKeyType::User);
        let ip_limit = pub_limit(Method::PUT, "1mb/s", LimitKeyType::Ip);
        let request_limit = pub_limit(Method::PUT, "10r/m", LimitKeyType::User);

        let write_rate: BandwidthRate = "5mb/s".parse().unwrap();
        let read_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let quota = UserResourceQuota {
            rate_write: QuotaOverride::Value(write_rate.clone()),
            rate_read: QuotaOverride::Value(read_rate.clone()),
            ..Default::default()
        };

        // User-keyed PUT speed limit → should return Custom(rate_write)
        assert!(matches!(
            write_limit.user_rate_override(&quota),
            Some(RateOverride::Custom(r)) if *r == write_rate
        ));

        // User-keyed GET speed limit → should return Custom(rate_read)
        assert!(matches!(
            read_limit.user_rate_override(&quota),
            Some(RateOverride::Custom(r)) if *r == read_rate
        ));

        // IP-keyed speed limit → no override
        assert!(ip_limit.user_rate_override(&quota).is_none());

        // Request-count limit → no override
        assert!(request_limit.user_rate_override(&quota).is_none());
    }

    #[tokio::test]
    async fn test_user_rate_override_unlimited_bypass() {
        use crate::data_directory::user_resource_quota::QuotaOverride;

        let write_limit = pub_limit(Method::PUT, "1mb/s", LimitKeyType::User);
        let quota = UserResourceQuota {
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };

        assert!(matches!(
            write_limit.user_rate_override(&quota),
            Some(RateOverride::Bypass)
        ));

        // resolve_effective_limiter returns None for bypass
        assert!(write_limit
            .resolve_effective_limiter(Some(&quota))
            .is_none());
    }

    #[tokio::test]
    async fn test_user_without_override_uses_default() {
        let limit = pub_limit(Method::PUT, "1mb/s", LimitKeyType::User);

        // User with no rate overrides (all Default)
        let quota = UserResourceQuota::default();
        assert!(limit.user_rate_override(&quota).is_none());

        // resolve_effective_limiter returns Some (the default limiter)
        assert!(limit.resolve_effective_limiter(Some(&quota)).is_some());
    }

    #[tokio::test]
    async fn test_override_limiter_creation() {
        let limit = pub_limit(Method::PUT, "1mb/s", LimitKeyType::User);

        let rate: BandwidthRate = "5mb/s".parse().unwrap();
        let limiter1 = limit.get_or_create_override_limiter(&rate);
        let limiter2 = limit.get_or_create_override_limiter(&rate);

        // Same rate should return the same limiter instance
        assert!(Arc::ptr_eq(&limiter1, &limiter2));

        // Different rate should return a different limiter
        let other_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let limiter3 = limit.get_or_create_override_limiter(&other_rate);
        assert!(!Arc::ptr_eq(&limiter1, &limiter3));
    }
}
