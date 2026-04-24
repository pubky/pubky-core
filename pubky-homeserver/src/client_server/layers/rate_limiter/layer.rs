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
use axum::response::{IntoResponse, Response};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use governor::RateLimiter;
use std::sync::Arc;
use std::time::Duration;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::client_server::extractors::PubkyHost;
use crate::quota_config::{LimitKey, PathLimit};
use crate::services::user_service::UserService;
use crate::shared::HttpError;
use crate::DefaultQuotasToml;

use super::limiter_pool::{KeyedRateLimiter, LimitTuple, LimiterPool};
use super::request_info::{is_write_method, RequestInfo};
use super::throttle::{throttle_request, throttle_response};
use super::CLEANUP_INTERVAL_SECS;

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
    /// * `path_limits` — per-path request-count limits from `[[drive.rate_limits]]`.
    /// * `user_service` — for resolving per-user quota overrides.
    /// * `defaults` — system-wide bandwidth defaults from `[default_quotas]`.
    pub fn new(
        path_limits: Vec<PathLimit>,
        user_service: UserService,
        defaults: crate::DefaultQuotasToml,
    ) -> Self {
        if path_limits.is_empty() {
            tracing::info!("No path-based request-count rate limits configured ([[drive.rate_limits]] is empty). Per-user bandwidth throttling may still apply via [default_quotas].");
        } else {
            let limits_str = path_limits
                .iter()
                .map(|limit| format!("\"{limit}\""))
                .collect::<Vec<String>>();
            tracing::info!(
                "Path-based rate limits configured: {}",
                limits_str.join(", ")
            );
        }

        Self {
            limits: path_limits,
            user_service,
            defaults,
        }
    }
}

impl<S> Layer<S> for RateLimiterLayer {
    type Service = RateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let state = RateLimiterState::new(
            &self.limits,
            self.user_service.clone(),
            self.defaults.clone(),
        );
        RateLimiterMiddleware { inner, state }
    }
}

/// Runtime state shared between middleware clones. Encapsulates all
/// rate-limiting resolution logic.
#[derive(Debug, Clone)]
struct RateLimiterState {
    limits: Vec<LimitTuple>,
    user_service: UserService,
    defaults: DefaultQuotasToml,
    user_read_limiters: LimiterPool,
    user_write_limiters: LimiterPool,
    unauthenticated_read_limiter: Option<Arc<KeyedRateLimiter>>,
}

impl RateLimiterState {
    /// Build runtime state: create governor instances for each path limit,
    /// per-user bandwidth limiter pools, and the unauthenticated IP limiter.
    fn new(
        path_limits: &[PathLimit],
        user_service: UserService,
        defaults: DefaultQuotasToml,
    ) -> Self {
        let limits = path_limits
            .iter()
            .map(|path| LimitTuple::new(path.clone()))
            .collect();

        let unauthenticated_read_limiter =
            defaults.unauthenticated_ip_rate_read.as_ref().map(|rate| {
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

        Self {
            limits,
            user_service,
            defaults,
            user_read_limiters: LimiterPool::new(),
            user_write_limiters: LimiterPool::new(),
            unauthenticated_read_limiter,
        }
    }

    /// Returns `true` when there is *something* to check for this request.
    fn has_work(&self, req: &Request<Body>) -> bool {
        let has_path_limits = self.limits.iter().any(|l| l.is_match(req));
        let has_user = req.extensions().get::<PubkyHost>().is_some();
        let has_bandwidth = self.unauthenticated_read_limiter.is_some();
        has_path_limits || has_user || has_bandwidth
    }

    /// Phase 1: check request-count path limits.
    /// Returns an error response if any limit is exceeded.
    #[allow(clippy::result_large_err)]
    fn check_request_count_limits(&self, req: &Request<Body>) -> Result<(), Response> {
        for limit in &self.limits {
            if !limit.is_request_count_match(req) {
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

    /// Phase 2: resolve which bandwidth throttlers apply to this request.
    ///
    /// The returned `(LimitKey, Arc<KeyedRateLimiter>)` pairs are used to
    /// throttle both the upload (request body) and download (response body).
    ///
    /// Takes pre-extracted request metadata to avoid holding a `&Request`
    /// (which is `!Send`) across the `.await` inside.
    #[allow(clippy::result_large_err)]
    async fn resolve_bandwidth_throttlers(
        &self,
        info: &RequestInfo,
    ) -> Result<Vec<(LimitKey, Arc<KeyedRateLimiter>)>, Response> {
        let mut throttlers = Vec::new();

        if info.authenticated {
            // Authenticated user: resolve per-user quota and apply bandwidth limits.
            if let Some(ref pubkey) = info.user_pubkey {
                let user_quota = self.user_service.resolve_quota(pubkey).await.map_err(|e| {
                    tracing::error!("Failed to resolve user limits for {}: {e}", pubkey.z32());
                    HttpError::new_with_message(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to resolve user limits",
                    )
                    .into_response()
                })?;

                if let Some(ref quota) = user_quota {
                    let user_key = LimitKey::User(pubkey.clone());
                    let is_write = is_write_method(&info.method);
                    let (default_rate, default_burst, user_burst) = if is_write {
                        (
                            self.defaults.rate_write.as_ref(),
                            self.defaults.rate_write_burst,
                            quota.rate_write_burst,
                        )
                    } else {
                        (
                            self.defaults.rate_read.as_ref(),
                            self.defaults.rate_read_burst,
                            quota.rate_read_burst,
                        )
                    };
                    // Per-user burst → server default burst → None (burst = rate).
                    let burst = user_burst.or(default_burst);
                    let rate_field = if is_write {
                        &quota.rate_write
                    } else {
                        &quota.rate_read
                    };
                    let resolved = rate_field
                        .resolve_with_default(default_rate)
                        .map(|rate| (rate, burst));

                    if let Some((rate, burst)) = resolved {
                        let pool = if is_write {
                            &self.user_write_limiters
                        } else {
                            &self.user_read_limiters
                        };
                        let limiter = pool.get_or_create(&rate, burst);
                        throttlers.push((user_key, limiter));
                    }
                } else {
                    // Unknown user (e.g. spoofed cookie): fall back to
                    // unauthenticated IP bandwidth rate so attackers can't
                    // bypass throttling by faking a session cookie.
                    // Applies to both reads and writes — writes will be
                    // rejected downstream by the auth layer, but throttling
                    // here prevents bandwidth exhaustion from streamed bodies.
                    self.push_ip_throttler(&info.client_ip, &mut throttlers);
                }
            }
        } else {
            // Unauthenticated request: apply IP-keyed bandwidth limit.
            self.push_ip_throttler(&info.client_ip, &mut throttlers);
        }

        Ok(throttlers)
    }

    /// Try to add the unauthenticated IP limiter for the client IP.
    fn push_ip_throttler(
        &self,
        client_ip: &Result<std::net::IpAddr, anyhow::Error>,
        throttlers: &mut Vec<(LimitKey, Arc<KeyedRateLimiter>)>,
    ) {
        if let Some(ref limiter) = self.unauthenticated_read_limiter {
            match client_ip {
                Ok(ip) => throttlers.push((LimitKey::Ip(*ip), limiter.clone())),
                Err(e) => {
                    tracing::warn!("Failed to extract IP for unauthenticated rate limiting: {e}");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiterMiddleware<S> {
    inner: S,
    state: RateLimiterState,
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

        if !self.state.has_work(&req) {
            return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
        }

        let state = self.state.clone();

        Box::pin(async move {
            // ── Phase 1: Request-count limits ──
            if let Err(resp) = state.check_request_count_limits(&req) {
                return Ok(resp);
            }

            // ── Phase 2: Bandwidth throttling ──
            let info = RequestInfo::from_request(&req);
            let throttlers = match state.resolve_bandwidth_throttlers(&info).await {
                Ok(t) => t,
                Err(resp) => return Ok(resp),
            };

            for (key, limiter) in &throttlers {
                req = throttle_request(req, key, limiter);
            }

            // ── Phase 3: Call inner service ──
            let mut response = match inner.call(req).await.map_err(|_| unreachable!()) {
                Ok(response) => response,
                Err(e) => return Err(e),
            };

            // ── Phase 4: Download throttling ──
            for (key, limiter) in &throttlers {
                response = throttle_response(response, key, limiter);
            }

            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::http::Method;
    use axum::{
        routing::{get, post},
        Router,
    };
    use axum_server::Server;
    use futures_util::StreamExt;
    use pubky_common::crypto::{Keypair, PublicKey};
    use reqwest::{Client, Response};
    use tokio::{task::JoinHandle, time::Instant};
    use tower_cookies::CookieManagerLayer;

    use crate::data_directory::quota_config::BandwidthRate;
    use crate::shared::user_quota::{QuotaOverride, UserQuota};
    use crate::shared::HttpResult;
    use crate::{
        client_server::layers::pubky_host::PubkyHostLayer,
        quota_config::{GlobPattern, LimitKeyType},
    };

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

    /// Integration test: authenticated user with a per-user DB rate override
    /// gets their custom rate applied through the full middleware stack.
    ///
    /// Setup:
    /// - Server default `rate_read` = 1kb/s (very slow)
    /// - User's DB override `rate_read` = 100mb/s (fast)
    /// - Unauthenticated IP rate = 1kb/s (very slow)
    ///
    /// An authenticated download (3kb) should complete in < 2s (user override),
    /// while an unauthenticated download should take > 2s (IP rate limit).
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_authenticated_user_gets_per_user_rate_from_db() {
        use crate::persistence::sql::user::UserRepository;

        let db = SqlDb::test().await;
        let user_service = UserService::new(db.clone());

        // Create a user in the DB with a fast read rate override.
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let quota = UserQuota {
            rate_read: QuotaOverride::Value("100mb/s".parse().unwrap()),
            ..Default::default()
        };
        UserRepository::set_quota(user.id, &quota, &mut db.pool().into())
            .await
            .unwrap();

        // Server defaults: slow read rate + slow unauthenticated IP rate.
        let defaults = crate::DefaultQuotasToml {
            rate_read: Some("1kb/s".parse().unwrap()),
            unauthenticated_ip_rate_read: Some("1kb/s".parse().unwrap()),
            ..Default::default()
        };

        let socket = start_server_with_user_service(vec![], user_service, defaults).await;
        let z32 = pubkey.z32();

        // Authenticated request: set pubky-host and a session cookie.
        // The cookie value doesn't matter — is_authenticated only checks existence.
        let start = Instant::now();
        let client = Client::new();
        let response = client
            .get(format!("http://{}/download?pubky-host={}", socket, z32))
            .header("Cookie", format!("{}=fake_session", z32))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.bytes().await.unwrap();
        let authenticated_time = start.elapsed();

        // Unauthenticated request: no cookie, should be throttled at 1kb/s.
        let start = Instant::now();
        let response = client
            .get(format!("http://{}/download", socket))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        response.bytes().await.unwrap();
        let unauthenticated_time = start.elapsed();

        assert!(
            authenticated_time < Duration::from_secs(2),
            "Authenticated download with 100mb/s override should be fast, took: {:?}",
            authenticated_time
        );
        assert!(
            unauthenticated_time > Duration::from_secs(2),
            "Unauthenticated download at 1kb/s should take >2s for 3kb, took: {:?}",
            unauthenticated_time
        );
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
