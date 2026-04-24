//! Authorization middleware.
//!
//! Enforces access control on every request:
//! - **Reads** to `/pub/*` are always allowed (public data).
//! - **Writes** (`PUT`/`DELETE`) require a valid session cookie whose capabilities
//!   grant write access to the target path and whose user matches the tenant.
//! - Non-public paths are forbidden for external requests.
//!
//! When a request has a valid session, an [`AuthenticatedSession`] marker is
//! inserted into the request extensions so downstream layers (e.g. per-user
//! rate limiting) can distinguish authenticated from anonymous requests.

use crate::client_server::{extractors::PubkyHost, AppState};
use crate::persistence::sql::session::{SessionRepository, SessionSecret};
use crate::shared::{HttpError, HttpResult};
use axum::http::Method;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pubky_common::crypto::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

/// Marker inserted into request extensions when the request has a valid session.
/// Downstream middleware (e.g. per-user rate limiting) can check for this to
/// distinguish authenticated from anonymous requests.
#[derive(Debug, Clone)]
pub struct AuthenticatedSession;

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

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let state = self.state.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path();

            let pubky = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk,
                None => {
                    tracing::warn!("Pubky Host is missing in request. Authorization failed.");
                    return Ok(HttpError::new_with_message(
                        StatusCode::NOT_FOUND,
                        "Pubky Host is missing",
                    )
                    .into_response());
                }
            };

            let cookies = match req.extensions().get::<Cookies>() {
                Some(cookies) => cookies,
                None => {
                    tracing::warn!("No cookies found in request. Unauthorized.");
                    return Ok(HttpError::unauthorized().into_response());
                }
            };

            // Authorize the request
            match authorize(&state, req.method(), cookies, pubky.public_key(), path).await {
                Err(e) => return Ok(e.into_response()),
                Ok(true) => {
                    // Session was validated — mark as authenticated for downstream layers.
                    req.extensions_mut().insert(AuthenticatedSession);
                }
                Ok(false) => {
                    // Allowed without session (e.g. public read). Check if there
                    // happens to be a valid session anyway so downstream layers
                    // (per-user rate limiting) can offer authenticated-user quotas.
                    if has_valid_session(&state, cookies, pubky.public_key()).await {
                        req.extensions_mut().insert(AuthenticatedSession);
                    }
                }
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize the request. Returns `Ok(true)` when a session was validated,
/// `Ok(false)` when the request is allowed without a session (e.g. public read).
async fn authorize(
    state: &AppState,
    method: &Method,
    cookies: &Cookies,
    public_key: &PublicKey,
    path: &str,
) -> HttpResult<bool> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(false);
    } else if path.starts_with("/pub/") {
        if method == Method::GET || method == Method::HEAD {
            return Ok(false);
        }
    } else if path.starts_with("/dav/") {
        // XXX: at least for now
        // if method == Method::GET {
        //     return Ok(false);
        // }
    } else {
        tracing::warn!(
            "Writing to directories other than '/pub/' is forbidden: {}/{}. Access forbidden",
            public_key,
            path
        );
        return Err(HttpError::forbidden_with_message(
            "Writing to directories other than '/pub/' is forbidden",
        ));
    }

    let session_secret = match session_secret_from_cookies(cookies, public_key) {
        Some(session_secret) => session_secret,
        None => {
            tracing::warn!(
                "No session secret found in cookies for pubky-host: {}",
                public_key
            );
            return Err(HttpError::unauthorized_with_message(
                "No session secret found in cookies",
            ));
        }
    };

    let session =
        match SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into())
            .await
        {
            Ok(session) => session,
            Err(sqlx::Error::RowNotFound) => {
                tracing::warn!(
                    "No session found in the database for session secret: {}, pubky: {}",
                    session_secret,
                    public_key
                );
                return Err(HttpError::unauthorized_with_message(
                    "No session found for session secret",
                ));
            }
            Err(e) => return Err(e.into()),
        };

    if &session.user_pubkey != public_key {
        tracing::warn!(
            "SessionInfo public key does not match pubky-host: {} != {}",
            session.user_pubkey,
            public_key
        );
        return Err(HttpError::unauthorized_with_message(
            "SessionInfo public key does not match pubky-host",
        ));
    }

    if session.capabilities.iter().any(|cap| {
        path.starts_with(&cap.scope)
            && cap
                .actions
                .contains(&pubky_common::capabilities::Action::Write)
    }) {
        Ok(true)
    } else {
        tracing::warn!(
            "SessionInfo {} pubkey {} does not have write access to {}. Access forbidden",
            session_secret,
            public_key,
            path
        );
        Err(HttpError::forbidden_with_message(
            "Session does not have write access to path",
        ))
    }
}

/// Lightweight check: does the request carry a valid session cookie for this pubkey?
/// Only performs a DB lookup if a session cookie is actually present, so anonymous
/// requests incur no overhead.
async fn has_valid_session(state: &AppState, cookies: &Cookies, public_key: &PublicKey) -> bool {
    let Some(session_secret) = session_secret_from_cookies(cookies, public_key) else {
        return false;
    };
    match SessionRepository::get_by_secret(&session_secret, &mut state.sql_db.pool().into()).await {
        Ok(session) => &session.user_pubkey == public_key,
        Err(_) => false,
    }
}

/// Get the session secret from the cookies.
/// Returns None if the session secret is not found or invalid.
pub fn session_secret_from_cookies(
    cookies: &Cookies,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    let value = cookies
        .get(&public_key.z32())
        .map(|c| c.value().to_string())?;
    SessionSecret::new(value).ok()
}
