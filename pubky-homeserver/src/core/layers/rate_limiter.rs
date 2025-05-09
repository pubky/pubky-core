//!
//! Implements rate limiting with governor.
//!
//! Would love to use tower_governor but I can't type it properly due to
//! https://github.com/benwis/tower-governor/issues/49.
//!
//! So we implement our own rate limiter here.
//! Maybe one day we can switch to tower_governor if the issue is fixed.
//!

use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use governor::clock::QuantaClock;
use governor::state::keyed::DashMapStateStore;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use crate::core::error::Result;
use crate::core::extractors::PubkyHost;
use crate::core::Error;
use crate::quota_config::{LimitKey, QuotaConfig, RateUnit};
use futures_util::StreamExt;
use governor::{Jitter, RateLimiter};

/// A Tower Layer to handle general rate limiting.
/// 
/// Supports rate limiting by request count and by upload speed.
/// 
/// The key for the rate limiter can be the ip address or the user pubkey.
#[derive(Debug, Clone)]
pub struct RateLimiterLayer {
    config: Option<QuotaConfig>,
}

impl RateLimiterLayer {
    /// Create a new rate limiter layer with the given quota.
    /// 
    /// If quota is None, rate limiting is disabled.
    pub fn new(quota: Option<QuotaConfig>) -> Self {
        Self { config: quota }
    }
}

impl<S> Layer<S> for RateLimiterLayer {
    type Service = RateLimiterMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        let values = self.config.clone().map(|config| LimiterValues {
            limiter: Arc::new(RateLimiter::keyed(governor::Quota::from(
                config.quota_value.clone(),
            ))),
            config,
        });
        RateLimiterMiddleware { inner, values }
    }
}


/// Just a simple struct to store the limiter and the config.
#[derive(Debug, Clone)]
struct LimiterValues {
    pub config: QuotaConfig,
    pub limiter: Arc<RateLimiter<String, DashMapStateStore<String>, QuantaClock>>,
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct RateLimiterMiddleware<S> {
    inner: S,
    values: Option<LimiterValues>,
}

impl<S> RateLimiterMiddleware<S> {
    /// Extract the key from the request.
    /// 
    /// The key is the ip address of the client
    /// or the user pubkey.
    fn extract_key(&self, req: &Request<Body>, values: &LimiterValues) -> Option<String> {
        match values.config.limit_key {
            LimitKey::Ip => {
                // Extract the ip address from the request.
                let headers = req.headers();
                maybe_x_forwarded_for(headers)
                    .or_else(|| maybe_x_real_ip(headers))
                    .or_else(|| maybe_connect_info(&req))
                    .map(|ip| ip.to_string())
            }
            LimitKey::User => {
                // Extract the user pubkey from the request.
                req.extensions().get::<PubkyHost>().map(|pk| pk.public_key().to_string())

            }
        }

    }

    /// Throttle the upload body.
    ///
    /// Important: The speed quotas are always in kilobytes, not bytes.
    /// Counting bytes is not practical.
    ///
    fn throttle_upload(
        &self,
        req: Request<Body>,
        key: &String,
        limiter: &Arc<RateLimiter<String, DashMapStateStore<String>, QuantaClock>>,
    ) -> Request<Body> {
        let (parts, body) = req.into_parts();

        let body_stream = body.into_data_stream();
        let limiter = limiter.clone();
        let key = key.clone();
        let throttled = body_stream
            .map(move |chunk| {
                let limiter = limiter.clone();
                let key = key.clone();
                let jitter = Jitter::new(
                    Duration::from_millis(25),
                    Duration::from_millis(500),
                );
                async move {
                    match chunk {
                        Ok(actual_chunk) => {
                            let kilobytes = actual_chunk.len().div_ceil(1024);
                            for _ in 0..kilobytes {
                                // Check each kilobyte
                                if let Err(_) = limiter
                                    .until_key_n_ready_with_jitter(
                                        &key,
                                        NonZero::new(1).expect("1 is always non zero"),
                                        jitter,
                                    )
                                    .await
                                {
                                    unreachable!("Rate limiting is based on the number of kilobytes, not bytes. So 1 kb should always be allowed.");
                                };
                            }
                            Ok(actual_chunk)
                        }
                        Err(e) => Err(e),
                    }
                }
            })
            .buffered(1);

        let new_body = Body::from_stream(throttled);
        Request::from_parts(parts, new_body)
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

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let mut inner = self.inner.clone();

        let values = match &self.values {
            Some(values) => values,
            None => {
                // No rate limiting is enabled, so we can just call the next layer.
                return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
            }
        };

        let key = match self.extract_key(&req, values) {
            Some(key) => key,
            None => {
                tracing::warn!("Failed to extract ip from header for rate limiting.");
                return Box::pin(async move {
                    Ok(Error::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Some(
                            "Failed to extract key (ip or user pubkey) for rate limiting."
                                .to_string(),
                        ),
                    )
                    .into_response())
                });
            }
        };

        match values.config.quota_value.rate_unit {
            RateUnit::SpeedRateUnit(_) => {
                // Speed limiting is enabled, so we need to throttle the upload.
                let req = self.throttle_upload(req, &key, &values.limiter);
                return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
            }
            RateUnit::Request => {
                // Request limiting is enabled, so we need to limit the number of requests.
                match values.limiter.check_key(&key) {
                    Ok(()) => {
                        // Rate limit not exceeded, call the next layer
                        return Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) });
                    }
                    Err(e) => {
                        tracing::warn!("Rate limit exceeded for {key}: {}", e);
                        return Box::pin(async move {
                            Ok(Error::new(
                                StatusCode::TOO_MANY_REQUESTS,
                                Some("rate limit exceeded".to_string()),
                            )
                            .into_response())
                        });
                    }
                };
                
            }
        };
    }
}

// From https://github.com/benwis/tower-governor/blob/main/src/key_extractor.rs#L121

const X_REAL_IP: &str = "x-real-ip";
const X_FORWARDED_FOR: &str = "x-forwarded-for";

/// Tries to parse the `x-forwarded-for` header
fn maybe_x_forwarded_for(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_FORWARDED_FOR)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.split(',').find_map(|s| s.trim().parse::<IpAddr>().ok()))
}

/// Tries to parse the `x-real-ip` header
fn maybe_x_real_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get(X_REAL_IP)
        .and_then(|hv| hv.to_str().ok())
        .and_then(|s| s.parse::<IpAddr>().ok())
}

fn maybe_connect_info<T>(req: &Request<T>) -> Option<IpAddr> {
    req.extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|addr| addr.ip())
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use axum::{routing::post, Router};
    use axum_server::Server;
    use pkarr::{Keypair, PublicKey};
    use reqwest::{Client, Response};
    use tokio::{task::JoinHandle, time::Instant};

    use crate::core::layers::pubky_host::PubkyHostLayer;

    use super::*;

    // Fake upload handler that just consumes the body.
    pub async fn upload_handler(body: Body) -> Result<impl IntoResponse> {
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await.transpose()? {
            // Consume body
            let _ = chunk;
        }
        Ok((StatusCode::CREATED, ()))
    }

    // Start a server with the given quota config on a random port.
    async fn start_server(config: QuotaConfig) -> SocketAddr {
        let app = Router::new().route(
            "/upload",
            post(upload_handler).layer(RateLimiterLayer::new(Some(config))).layer(PubkyHostLayer),
        );

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
        let server = Server::from_tcp(listener.into_std().unwrap());

        tokio::spawn(async move {
            server
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .unwrap();
        });

        socket
    }

    #[tokio::test]
    async fn test_throttle_upload() {
        let config: QuotaConfig = "ip:1kb/s".parse().unwrap();
        let socket = start_server(config).await;

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
        let handle1 = upload_data(socket, 3);
        let handle2 = upload_data(socket, 3);

        // Wait for the uploads to finish
        let _ = tokio::try_join!(handle1, handle2);

        let time_taken = start.elapsed();
        assert!(time_taken > Duration::from_secs(6), "Should at least take 6s because uploads are limited to 1kb/s and the sum of the uploads is 6kb");
        println!("Time taken: {:?}", time_taken);
    }

    #[tokio::test]
    async fn test_limit_parallel_requests_with_ip_key() {
        let config: QuotaConfig = "ip:1r/m".parse().unwrap();
        let socket = start_server(config).await;

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
    async fn test_limit_parallel_requests_with_user_key() {
        let config: QuotaConfig = "user:1r/m".parse().unwrap();
        let socket = start_server(config).await;

        fn send_request(socket: SocketAddr, user_pubkey: PublicKey) -> JoinHandle<Response> {
            tokio::spawn(async move {
                let client = Client::new();
                let response = client
                    .post(format!("http://{}/upload?pubky-host={user_pubkey}", socket))
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
        let (
            res1, 
            res2,
            res3
        ) = tokio::try_join!(
            handle1, 
            handle2,
            handle3
        ).unwrap();
        assert_eq!(res1.status(), StatusCode::CREATED);
        assert_eq!(res2.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(res3.status(), StatusCode::CREATED);
    }
}
