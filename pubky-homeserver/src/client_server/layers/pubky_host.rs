use crate::client_server::extractors::PubkyHost;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use pubky_common::crypto::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

/// A Tower Layer to extract and inject the PubkyHost into request extensions.
#[derive(Debug, Clone)]
pub struct PubkyHostLayer;

impl<S> Layer<S> for PubkyHostLayer {
    type Service = PubkyHostLayerMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PubkyHostLayerMiddleware { inner }
    }
}

/// Middleware that extracts the public key from headers or query parameters.
#[derive(Debug, Clone)]
pub struct PubkyHostLayerMiddleware<S> {
    inner: S,
}

impl<S> Service<Request<Body>> for PubkyHostLayerMiddleware<S>
where
    S: Service<Request<Body>, Response = axum::response::Response, Error = Infallible>
        + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(|_| unreachable!())
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        if let Some(public_key) = extract_pubky(&req) {
            req.extensions_mut().insert(PubkyHost(public_key));
        }
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) })
    }
}

/// Extracts a PublicKey by checking, in order:
/// 1. The "host" header.
/// 2. The "pubky-host" header (which overwrites any previously found key).
/// 3. The query parameter "pubky-host" if none was found in headers.
fn extract_pubky(req: &Request<Body>) -> Option<PublicKey> {
    let mut pubky = None;
    // Check headers in order: "host" then "pubky-host".
    for header in ["host", "pubky-host"].iter() {
        if let Some(val) = req.headers().get(*header) {
            if let Ok(s) = val.to_str() {
                if is_prefixed_pubky(s) {
                    continue;
                }
                if let Ok(key) = PublicKey::try_from(s) {
                    pubky = Some(key);
                }
            }
        }
    }
    // If still no key, fall back to query parameter.
    if pubky.is_none() {
        pubky = req.uri().query().and_then(|query| {
            query.split('&').find_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                if let (Some(key), Some(val)) = (parts.next(), parts.next()) {
                    if key == "pubky-host" {
                        if is_prefixed_pubky(val) {
                            return None;
                        }
                        return PublicKey::try_from(val).ok();
                    }
                }
                None
            })
        });
    }
    pubky
}

fn is_prefixed_pubky(value: &str) -> bool {
    matches!(value.strip_prefix("pubky"), Some(stripped) if stripped.len() == 52)
}
