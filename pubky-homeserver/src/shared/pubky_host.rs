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

use crate::shared::HttpError;

/// A Tower Layer to extract and inject the PubkyHost into request extensions.
/// This is added to the router so the host is extracted automatically for each request.
/// You then can use `PubkyHost` extractor to get the host from the request.
/// Will return a 400 Bad Request if the host is not found or not a valid public key.
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
        let pubky_host = match extract_pubky_from_request(&req) {
            Ok(key) => key,
            Err(errors) => {
                return Box::pin(async move {
                    let error_message = errors.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", ");
                    let error_message = format!("Missing or invalid pubky_host header or query param:\n{}", error_message);
                    tracing::error!("Failed to extract PubkyHost: {}", error_message);
                    Ok(HttpError::new(StatusCode::BAD_REQUEST, Some(error_message)).into_response())
                });
            }
        };
        req.extensions_mut().insert(PubkyHost(pubky_host));
        
        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await.map_err(|_| unreachable!()) })
    }
}

#[derive(Debug)]
enum ExtractPubKeySource {
    Header,
    QueryParam,
}

impl std::fmt::Display for ExtractPubKeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) ->  std::fmt::Result {
        match self {
            ExtractPubKeySource::Header => write!(f, "Header"),
            ExtractPubKeySource::QueryParam => write!(f, "QueryParam"),
        }
    }
}

#[derive(Debug)]
struct ParamName(String);

impl Display for ParamName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(thiserror::Error, Debug)]
enum ExtractPubKeyError {
    #[error("{1} {0} not found")]
    NotFound(ParamName, ExtractPubKeySource),
    #[error("{1} {0} is not valid UTF-8")]
    InvalidUtf8(ParamName, ExtractPubKeySource),
    #[error("{1} {0} failed to parse public key: {2} {3}")]
    InvalidPublicKey(ParamName, ExtractPubKeySource, String, pkarr::errors::PublicKeyError),
}



/// Extracts a PublicKey from a header.
fn extract_pubky_from_header(req: &Request<Body>, header_name: &str) -> Result<PublicKey, ExtractPubKeyError> {
    let val = match req.headers().get(header_name) {
        Some(val) => val,
        None => return Err(ExtractPubKeyError::NotFound(ParamName(header_name.to_string()), ExtractPubKeySource::Header)),
    };
    let val_str = match val.to_str() {
        Ok(val_str) => val_str,
        Err(_e) => return Err(ExtractPubKeyError::InvalidUtf8(ParamName(header_name.to_string()), ExtractPubKeySource::Header)),
    };
    let key = PublicKey::try_from(val_str).map_err(|e| ExtractPubKeyError::InvalidPublicKey(ParamName(header_name.to_string()), ExtractPubKeySource::Header, val_str.to_string(), e))?;
    Ok(key)
}

/// Extracts a PublicKey from a query parameter.
fn extract_pubky_from_query_param(req: &Request<Body>, query_name: &str) -> Result<PublicKey, ExtractPubKeyError> {
    let query = req.uri().query().ok_or(ExtractPubKeyError::NotFound(ParamName(query_name.to_string()), ExtractPubKeySource::QueryParam))?;
    let mut key_values = query.split('&').filter_map(|pair| {
        let parts = pair.split('=').collect::<Vec<_>>();
        if parts.len() != 2 {
            return None;
        }
        return Some((parts[0], parts[1]));
    });

    let target_key_value = key_values.find(|(key, _)| *key == query_name);
    let target_value = match target_key_value {
        Some((_, val)) => val,
        None => return Err(ExtractPubKeyError::NotFound(ParamName(query_name.to_string()), ExtractPubKeySource::QueryParam)),
    };

    let key = PublicKey::try_from(target_value)
    .map_err(|e| ExtractPubKeyError::InvalidPublicKey(ParamName(query_name.to_string()), ExtractPubKeySource::QueryParam, target_value.to_string(), e))?;
    Ok(key)
}

/// Extracts a PublicKey by checking, in order:
/// 1. The "pubky-host" header.
/// 2. The "host" header.
/// 3. The query parameter "pubky-host".
fn extract_pubky_from_request(req: &Request<Body>) -> Result<PublicKey, Vec<ExtractPubKeyError>> {
    let mut errors = vec![];
    // Check headers in order: "host" then "pubky-host".
    for header in ["pubky-host", "host"].iter() {
        match extract_pubky_from_header(req, header) {
            Ok(key) => {
                return Ok(key);
            }
            Err(e) => errors.push(e),
        }
    }

    match extract_pubky_from_query_param(req, "pubky-host") {
        Ok(key) => {
            return Ok(key);
        }
        Err(e) => errors.push(e),
    }
    
    Err(errors)
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
            .map_err(|e| {
                tracing::debug!("Failed to extract PubkyHost for {} {}.", parts.method, parts.uri);
                e.into_response()
            })?;

        Ok(pubky_host)
    }
}
