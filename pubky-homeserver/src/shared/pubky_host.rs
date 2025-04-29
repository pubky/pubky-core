use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use std::fmt::Display;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};

/// A Tower Layer to extract and inject the PubkyHost into request extensions.
/// This is added to the router so the host is extracted automatically for each request.
/// You then can use `PubkyHost` extractor to get the host from the request.
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
                        return PublicKey::try_from(val).ok();
                    }
                }
                None
            })
        });
    }
    pubky
}

/// Extractor for the PubkyHost.
#[derive(Debug, Clone)]
pub struct PubkyHost(pub(crate) PublicKey);

impl PubkyHost {
    pub fn public_key(&self) -> &PublicKey {
        &self.0
    }
}

impl Display for PubkyHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<S> FromRequestParts<S> for PubkyHost
where
    S: Sync + Send,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let pubky_host = parts
            .extensions
            .get::<PubkyHost>()
            .cloned()
            .ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Can't extract PubkyHost. Is `PubkyHostLayer` enabled?",
            ))
            .map_err(|e| e.into_response())?;

        Ok(pubky_host)
    }
}
