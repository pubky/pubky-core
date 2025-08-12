use crate::core::extractors::PubkyHost;
use axum::{body::Body, http::Request};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
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

/// Extracts a PublicKey from the query parameter "pubky-host".
fn extract_pubky(req: &Request<Body>) -> Option<PublicKey> {
    let pubky = req.uri().query().and_then(|query| {
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

    pubky
}
