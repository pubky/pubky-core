use axum::http::{header, HeaderMap, Method};
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

use crate::core::{
    error::{Error, Result},
    extractors::PubkyHost,
    AppState,
};

/// A Tower Layer to handle authorization for write operations.
#[derive(Debug, Clone)]
pub struct AuthorizationLayer {
    state: AppState,
}

impl AuthorizationLayer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for AuthorizationLayer {
    type Service = AuthorizationMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthorizationMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

/// Middleware that performs authorization checks for write operations.
#[derive(Debug, Clone)]
pub struct AuthorizationMiddleware<S> {
    inner: S,
    state: AppState,
}

impl<S> Service<Request<Body>> for AuthorizationMiddleware<S>
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
        let state = self.state.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path();

            // Verify the path
            if let Err(e) = verify(path) {
                return Ok(e.into_response());
            }

            let pubky = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk,
                None => {
                    return Ok(
                        Error::new(StatusCode::NOT_FOUND, "Pubky Host is missing".into())
                            .into_response(),
                    )
                }
            };

            // Authorize the request
            if let Err(e) = authorize(
                &state,
                req.method(),
                req.headers(),
                pubky.public_key(),
                path,
            ) {
                return Ok(e.into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Verifies the path.
fn verify(path: &str) -> Result<()> {
    if !path.starts_with("/pub/") {
        return Err(Error::new(
            StatusCode::FORBIDDEN,
            "Writing to directories other than '/pub/' is forbidden".into(),
        ));
    }
    Ok(())
}

/// Authorize write (PUT or DELETE) for Public paths.
fn authorize(
    state: &AppState,
    method: &Method,
    headers: &HeaderMap,
    public_key: &PublicKey,
    path: &str,
) -> Result<()> {
    if path.starts_with("/pub/") && method == Method::GET {
        return Ok(());
    }

    let session_secret = session_secret_from_headers(headers, public_key)
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    let session = state
        .db
        .get_session(&session_secret)?
        .ok_or(Error::with_status(StatusCode::UNAUTHORIZED))?;

    if session.pubky() == public_key
        && session.capabilities().iter().any(|cap| {
            path.starts_with(&cap.scope)
                && cap
                    .actions
                    .contains(&pubky_common::capabilities::Action::Write)
        })
    {
        return Ok(());
    }

    Err(Error::with_status(StatusCode::FORBIDDEN))
}

fn cookie_name(public_key: &PublicKey) -> String {
    public_key.to_string().chars().take(8).collect::<String>()
}

pub fn session_secret_from_cookies(cookies: Cookies, public_key: &PublicKey) -> Option<String> {
    cookies
        .get(&cookie_name(public_key))
        .map(|c| c.value().to_string())
}

// TODO: unit test this
fn session_secret_from_headers(headers: &HeaderMap, public_key: &PublicKey) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .find(|h| h.starts_with(&cookie_name(public_key)))
        .and_then(|h| {
            h.split(';')
                .next()
                .and_then(|key_value| key_value.split('=').last())
        })
        .map(|s| s.to_string())
}
