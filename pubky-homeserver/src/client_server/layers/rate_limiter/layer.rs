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
//!    path and method. Only request-count quotas are supported here.
//! 2. **Bandwidth config** (`[default_quotas]`):
//!    - `rate_read` / `rate_write` — defaults for authenticated users,
//!      with per-user DB overrides via `UserQuota`.
//!    - `unauthenticated_ip_rate_read` — for unauthenticated IP reads.
//!
//! # Auth detection
//!
//! Authentication is detected by checking for a session cookie matching the
//! PubkyHost (cheap cookie check, no DB hit). The full auth enforcement stays
//! in the tenant router.
//!
//! # Phases
//!
//! The `call()` method is structured into 4 phases:
//!
//! 1. **Request-count limits** — check path limits with `RateUnit::Request`,
//!    return 429 if exceeded.
//! 2. **Bandwidth throttling** — based on auth status:
//!    - Authenticated: resolve per-user rate via `UserService`, throttle upload.
//!    - Unauthenticated: apply `unauthenticated_ip_rate_read` (reads only).
//! 3. **Call inner service**.
//! 4. **Download throttling** — apply collected limiters to the response body.
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
use crate::quota_config::{LimitKey, LimitKeyType, PathLimit, RateUnit};
use crate::services::user_service::UserService;
use crate::shared::HttpError;
use crate::DefaultQuotasToml;
use futures_util::StreamExt;
use governor::{Jitter, Quota, RateLimiter};
use tower_cookies::Cookies;

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
    user_service: UserService,
    /// Default bandwidth rates from `[default_quotas]` config.
    defaults: DefaultQuotasToml,
}

impl RateLimiterLayer {
    /// Create a new rate limiter layer.
    ///
    /// * `limits` — per-path request-count limits from `[[drive.rate_limits]]`.
    /// * `user_service` — for resolving per-user quota overrides.
    /// * `defaults` — system-wide bandwidth defaults from `[default_quotas]`.
    pub fn new(
        limits: Vec<PathLimit>,
        user_service: UserService,
        defaults: crate::DefaultQuotasToml,
    ) -> Self {
        if limits.is_empty() {
            tracing::info!("General rate limiting is disabled.");
        } else {
            let limits_str = limits
                .iter()
                .map(|limit| format!("\"{limit}\""))
                .collect::<Vec<String>>();
            tracing::info!("Rate limits configured: {}", limits_str.join(", "));
        }

        Self {
            limits,
            user_service,
            defaults,
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

        let user_read_limiters = LimiterPool::new();
        let user_write_limiters = LimiterPool::new();
        let unauthenticated_read_limiter =
            self.defaults
                .unauthenticated_ip_rate_read
                .as_ref()
                .map(|rate| {
                    let quota = rate.to_governor_quota(None);
                    let limiter = Arc::new(RateLimiter::keyed(quota));

                    // Spawn cleanup task to evict expired IP entries.
                    let weak = Arc::downgrade(&limiter);
                    tokio::spawn(async move {
                        let mut interval =
                            tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
                        interval.tick().await; // skip first immediate tick
                        loop {
                            interval.tick().await;
                            let Some(limiter) = weak.upgrade() else {
                                break;
                            };
                            limiter.retain_recent();
                            limiter.shrink_to_fit();
                        }
                    });

                    limiter
                });

        RateLimiterMiddleware {
            inner,
            limits: tuples,
            user_service: self.user_service.clone(),
            defaults: self.defaults.clone(),
            user_read_limiters,
            user_write_limiters,
            unauthenticated_read_limiter,
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

/// Shared pool of keyed rate limiters, grouped by (rate, burst).
///
/// Users with the same configured rate and burst share a single governor
/// instance, keyed by their public key.
#[derive(Debug, Clone)]
struct LimiterPool(Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>>);

impl LimiterPool {
    /// Create a new empty pool and spawn a background cleanup task.
    /// The cleanup task self-terminates when the Arc is dropped (Weak::upgrade fails).
    fn new() -> Self {
        let inner: Arc<DashMap<SpeedLimitKey, Arc<KeyedRateLimiter>>> = Arc::new(DashMap::new());

        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
            interval.tick().await; // skip first immediate tick
            loop {
                interval.tick().await;
                let Some(pool) = weak.upgrade() else {
                    break;
                };
                pool.retain(|_, limiter| {
                    limiter.retain_recent();
                    limiter.shrink_to_fit();
                    !limiter.is_empty()
                });
            }
        });

        Self(inner)
    }

    /// Get or create a keyed rate limiter for a specific bandwidth rate + burst.
    fn get_or_create(&self, rate: &BandwidthRate, burst: Option<u32>) -> Arc<KeyedRateLimiter> {
        self.0
            .entry((rate.clone(), burst))
            .or_insert_with(|| {
                let quota: Quota = rate.to_governor_quota(burst);
                Arc::new(RateLimiter::keyed(quota))
            })
            .clone()
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiterMiddleware<S> {
    inner: S,
    limits: Vec<LimitTuple>,
    user_service: UserService,
    defaults: DefaultQuotasToml,
    user_read_limiters: LimiterPool,
    user_write_limiters: LimiterPool,
    unauthenticated_read_limiter: Option<Arc<KeyedRateLimiter>>,
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

/// Check request-count path limits. Returns an error response if any limit is exceeded.
#[allow(clippy::result_large_err)]
fn check_request_count_limits(
    req: &Request<Body>,
    path_limits: &[LimitTuple],
) -> Result<(), Response> {
    for limit in path_limits {
        if limit.limit.quota.rate_unit != RateUnit::Request {
            continue;
        }
        let key = match limit.extract_key(req) {
            Ok(key) => key,
            Err(e) => {
                tracing::warn!(
                    "{} {} Failed to extract key for rate limiting: {}",
                    limit.limit.path.0,
                    limit.limit.method.0,
                    e
                );
                return Err(HttpError::new_with_message(
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
            return Err(HttpError::new_with_message(
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded",
            )
            .into_response());
        }
    }
    Ok(())
}

/// Check if the request is authenticated by looking for a session cookie
/// matching the PubkyHost. This is a cheap cookie check with no DB hit.
fn is_authenticated(req: &Request<Body>) -> bool {
    req.extensions()
        .get::<Cookies>()
        .and_then(|cookies| {
            let pk = req.extensions().get::<PubkyHost>()?;
            cookies.get(&pk.public_key().z32())?;
            Some(())
        })
        .is_some()
}

/// Returns true for HTTP methods that represent writes (uploads).
fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::PUT | Method::POST | Method::PATCH | Method::DELETE
    )
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

        let has_bandwidth_config = self.unauthenticated_read_limiter.is_some();

        if path_limits.is_empty() && user_pubkey.is_none() && !has_bandwidth_config {
            // No limits matched and no user/bandwidth config, skip entirely.
            return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
        }

        let user_service = self.user_service.clone();
        let defaults = self.defaults.clone();
        let path_limits_owned: Vec<LimitTuple> = path_limits.into_iter().cloned().collect();
        let user_read_limiters = self.user_read_limiters.clone();
        let user_write_limiters = self.user_write_limiters.clone();
        let unauthenticated_read_limiter = self.unauthenticated_read_limiter.clone();

        Box::pin(async move {
            let mut download_throttlers: Vec<(LimitKey, Arc<KeyedRateLimiter>)> = Vec::new();

            // ── Phase 1: Request-count limits ──
            if let Err(resp) = check_request_count_limits(&req, &path_limits_owned) {
                return Ok(resp);
            }

            // ── Phase 2: Bandwidth throttling ──
            let authenticated = is_authenticated(&req);
            let method = req.method().clone();

            if authenticated {
                // Authenticated user: resolve per-user quota and apply bandwidth limits.
                if let Some(ref pubkey) = user_pubkey {
                    let user_quota = match user_service.resolve_quota(pubkey).await {
                        Ok(quota) => quota,
                        Err(e) => {
                            tracing::error!(
                                "Failed to resolve user limits for {}: {e}",
                                pubkey.z32()
                            );
                            return Ok(HttpError::new_with_message(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to resolve user limits",
                            )
                            .into_response());
                        }
                    };

                    if let Some(ref quota) = user_quota {
                        let user_key = LimitKey::User(pubkey.clone());
                        let is_write = is_write_method(&method);
                        let (default_rate, burst) = if is_write {
                            (defaults.rate_write.as_ref(), quota.rate_write_burst)
                        } else {
                            (defaults.rate_read.as_ref(), quota.rate_read_burst)
                        };
                        let rate_field = if is_write {
                            &quota.rate_write
                        } else {
                            &quota.rate_read
                        };
                        let resolved = rate_field
                            .resolve_with_default(default_rate)
                            .map(|rate| (rate, burst));

                        if let Some((rate, burst)) = resolved {
                            let limiter_pool = if is_write {
                                &user_write_limiters
                            } else {
                                &user_read_limiters
                            };
                            let limiter = limiter_pool.get_or_create(&rate, burst);
                            req = Self::throttle_upload(req, &user_key, &limiter);
                            download_throttlers.push((user_key, limiter));
                        }
                    } else {
                        // Unknown user (e.g. spoofed cookie): fall back to
                        // unauthenticated IP bandwidth rate so attackers can't
                        // bypass throttling by faking a session cookie.
                        // Applies to both reads and writes — writes will be
                        // rejected downstream by the auth layer, but throttling
                        // here prevents bandwidth exhaustion from streamed bodies.
                        if let Some(ref limiter) = unauthenticated_read_limiter {
                            if let Ok(ip) = extract_ip(&req) {
                                let key = LimitKey::Ip(ip);
                                req = Self::throttle_upload(req, &key, limiter);
                                download_throttlers.push((key, limiter.clone()));
                            }
                        }
                    }
                }
            } else if !is_write_method(&method) {
                // Unauthenticated read: apply IP-keyed bandwidth limit.
                if let Some(ref limiter) = unauthenticated_read_limiter {
                    match extract_ip(&req) {
                        Ok(ip) => {
                            let key = LimitKey::Ip(ip);
                            req = Self::throttle_upload(req, &key, limiter);
                            download_throttlers.push((key, limiter.clone()));
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Failed to extract IP for unauthenticated rate limiting: {e}"
                            );
                        }
                    }
                }
            }
            // Unauthenticated writes: no bandwidth throttling here;
            // the auth layer will reject them.

            // ── Phase 3: Call inner service ──
            let mut response = match inner.call(req).await.map_err(|_| unreachable!()) {
                Ok(response) => response,
                Err(e) => return Err(e),
            };

            // ── Phase 4: Download throttling ──
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
    use tower_cookies::CookieManagerLayer;

    use crate::shared::user_quota::{QuotaOverride, UserQuota};
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

    // Start a server with the given path limits and optional unauthenticated read rate.
    async fn start_server(
        config: Vec<PathLimit>,
        unauthenticated_ip_rate_read: Option<BandwidthRate>,
    ) -> SocketAddr {
        let db = SqlDb::test().await;
        let user_service = UserService::new(db);
        let defaults = crate::DefaultQuotasToml {
            unauthenticated_ip_rate_read,
            ..Default::default()
        };
        start_server_with_user_service(config, user_service, defaults).await
    }

    // Start a server with the given config and user service on a random port.
    async fn start_server_with_user_service(
        config: Vec<PathLimit>,
        user_service: UserService,
        defaults: crate::DefaultQuotasToml,
    ) -> SocketAddr {
        let app = Router::new()
            .route("/upload", post(upload_handler))
            .route("/download", get(download_handler))
            .layer(RateLimiterLayer::new(config, user_service, defaults))
            .layer(CookieManagerLayer::new())
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
    async fn test_throttle_unauthenticated_download() {
        // Unauthenticated IP read throttling via server-level config
        let rate: BandwidthRate = "1kb/s".parse().unwrap();
        let socket = start_server(vec![], Some(rate)).await;

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
        // Spawn 2 downloads in parallel. Download 3kb each at 1kb/s shared by IP.
        let handle1 = download_data(socket);
        let handle2 = download_data(socket);

        let _ = tokio::try_join!(handle1, handle2);

        let time_taken = start.elapsed();
        assert!(
            time_taken > Duration::from_secs(5),
            "Should take >5s: downloads limited to 1kb/s and sum is 6kb. Took: {:?}",
            time_taken
        );
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
        let socket = start_server(vec![path_limit], None).await;

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
        let socket = start_server(vec![path_limit], None).await;

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
        let write_rate: BandwidthRate = "5mb/s".parse().unwrap();
        let read_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let quota = UserQuota {
            rate_write: QuotaOverride::Value(write_rate.clone()),
            rate_read: QuotaOverride::Value(read_rate.clone()),
            ..Default::default()
        };

        // Resolved direction from HTTP method:
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
        let quota = UserQuota {
            rate_write: QuotaOverride::Unlimited,
            ..Default::default()
        };

        // Unlimited means no throttling is applied
        assert!(quota.rate_write.is_unlimited());
    }

    #[test]
    fn test_user_without_override_uses_server_default() {
        // User with no rate overrides (all Default) — falls back to
        // the server-level authenticated_rate_read/write config.
        let quota = UserQuota::default();
        assert!(quota.rate_write.is_default());
        assert!(quota.rate_read.is_default());

        // Test resolution via QuotaOverride directly (no DB needed)
        let default_read: BandwidthRate = "10mb/s".parse().unwrap();
        assert_eq!(
            quota.rate_read.resolve_with_default(Some(&default_read)),
            Some(default_read)
        );
        // No server default for write → None
        assert_eq!(quota.rate_write.resolve_with_default(None), None);
    }

    #[tokio::test]
    async fn test_user_limiter_pool_creation() {
        let pool = LimiterPool::new();

        let rate: BandwidthRate = "5mb/s".parse().unwrap();
        let limiter1 = pool.get_or_create(&rate, None);
        let limiter2 = pool.get_or_create(&rate, None);

        // Same rate + burst should return the same limiter instance
        assert!(Arc::ptr_eq(&limiter1, &limiter2));

        // Different rate should return a different limiter
        let other_rate: BandwidthRate = "10mb/s".parse().unwrap();
        let limiter3 = pool.get_or_create(&other_rate, None);
        assert!(!Arc::ptr_eq(&limiter1, &limiter3));

        // Same rate but different burst should return a different limiter
        let limiter4 = pool.get_or_create(&rate, Some(50));
        assert!(!Arc::ptr_eq(&limiter1, &limiter4));

        // Same rate + same burst should share
        let limiter5 = pool.get_or_create(&rate, Some(50));
        assert!(Arc::ptr_eq(&limiter4, &limiter5));
    }

    #[test]
    fn test_path_limit_rejects_bandwidth_quota() {
        let limit = PathLimit::new(
            GlobPattern::new("/pub/**"),
            Method::PUT,
            "1mb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        );
        let err = limit.validate().unwrap_err();
        assert!(
            err.to_string().contains("bandwidth quota"),
            "Expected bandwidth rejection error, got: {err}"
        );
    }

    #[test]
    fn test_path_limit_accepts_request_count_quota() {
        let limit = PathLimit::new(
            GlobPattern::new("/session"),
            Method::POST,
            "10r/m".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        );
        assert!(limit.validate().is_ok());
    }

    #[test]
    fn test_resolve_bandwidth_rate_with_user_override() {
        let default_read: BandwidthRate = "10mb/s".parse().unwrap();

        // User with custom read rate overrides the server default
        let custom_rate: BandwidthRate = "20mb/s".parse().unwrap();
        let quota = UserQuota {
            rate_read: QuotaOverride::Value(custom_rate.clone()),
            ..Default::default()
        };
        assert_eq!(
            quota.rate_read.resolve_with_default(Some(&default_read)),
            Some(custom_rate)
        );

        // User with Unlimited bypasses throttling
        let unlimited_quota = UserQuota {
            rate_read: QuotaOverride::Unlimited,
            ..Default::default()
        };
        assert_eq!(
            unlimited_quota
                .rate_read
                .resolve_with_default(Some(&default_read)),
            None
        );
    }
}
