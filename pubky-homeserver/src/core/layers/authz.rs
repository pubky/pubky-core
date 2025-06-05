use axum::http::Method;
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

use crate::core::{extractors::PubkyHost, AppState};
use crate::shared::{HttpError, HttpResult};

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

            let pubky = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk,
                None => {
                    return Ok(
                        HttpError::new(StatusCode::NOT_FOUND, Some("Pubky Host is missing"))
                            .into_response(),
                    )
                }
            };

            let cookies = req.extensions().get::<Cookies>();

            // Authorize the request
            if let Err(e) = authorize(&state, req.method(), cookies, pubky.public_key(), path) {
                return Ok(e.into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize write (PUT or DELETE) for Public paths.
fn authorize(
    state: &AppState,
    method: &Method,
    cookies: Option<&Cookies>,
    public_key: &PublicKey,
    path: &str,
) -> HttpResult<()> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(());
    } else if path.starts_with("/pub/") {
        if method == Method::GET {
            return Ok(());
        }
    } else {
        return Err(HttpError::new(
            StatusCode::FORBIDDEN,
            "Writing to directories other than '/pub/' is forbidden".into(),
        ));
    }

    if let Some(cookies) = cookies {
        let session_secret =
            session_secret_from_cookies(cookies, public_key).ok_or(HttpError::unauthorized())?;

        let session = state
            .db
            .get_session(&session_secret)?
            .ok_or(HttpError::unauthorized())?;

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

        return Err(HttpError::forbidden());
    }

    Err(HttpError::unauthorized())
}

pub fn session_secret_from_cookies(cookies: &Cookies, public_key: &PublicKey) -> Option<String> {
    cookies
        .get(&public_key.to_string())
        .map(|c| c.value().to_string())
}
