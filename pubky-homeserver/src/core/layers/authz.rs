use crate::core::{extractors::PubkyHost, AppState};
use crate::persistence::sql::session::{SessionRepository, SessionSecret};
use crate::shared::{HttpError, HttpResult};
use axum::http::Method;
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use std::{convert::Infallible, str::FromStr, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

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
            if let Err(e) = authorize(&state, req.method(), cookies, pubky.public_key(), path).await
            {
                return Ok(e.into_response());
            }

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize write (PUT or DELETE) for Public paths.
async fn authorize(
    state: &AppState,
    method: &Method,
    cookies: &Cookies,
    public_key: &PublicKey,
    path: &str,
) -> HttpResult<()> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(());
    } else if path.starts_with("/pub/") {
        if method == Method::GET || method == Method::HEAD {
            return Ok(());
        }
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

    let sessions = sessions_from_cookies(state, cookies, public_key).await?;

    // Check if any session has write access to the path
    for session in sessions {
        if session.capabilities.iter().any(|cap| {
            path.starts_with(&cap.scope)
                && cap
                    .actions
                    .contains(&pubky_common::capabilities::Action::Write)
        }) {
            // Found a valid session with required capabilities
            return Ok(());
        }
    }

    tracing::warn!(
        "No session with write access to {} found for pubky-host: {}",
        path,
        public_key
    );
    Err(HttpError::forbidden_with_message(
        "Session does not have write access to path",
    ))
}

/// Get all valid sessions from cookies that belong to the specified user.
///
/// Returns 401 if no session secrets found in cookies.
/// Returns 403 if cookies exist but no valid sessions found for the user.
/// Returns the list of valid sessions for the user.
pub async fn sessions_from_cookies(
    state: &AppState,
    cookies: &Cookies,
    public_key: &PublicKey,
) -> HttpResult<Vec<crate::persistence::sql::session::SessionEntity>> {
    let session_secrets = session_secrets_from_cookies(cookies);
    if session_secrets.is_empty() {
        tracing::warn!(
            "No session secret found in cookies for pubky-host: {}",
            public_key
        );
        return Err(HttpError::unauthorized_with_message(
            "No session secret found in cookies",
        ));
    }

    // Try each session secret and collect those that:
    // 1. Exist in the database
    // 2. Belong to the correct user
    let mut user_sessions = Vec::new();
    for session_secret in session_secrets {
        let session = match SessionRepository::get_by_secret(
            &session_secret,
            &mut state.sql_db.pool().into(),
        )
        .await
        {
            Ok(session) => session,
            Err(sqlx::Error::RowNotFound) => {
                continue;
            }
            Err(e) => return Err(e.into()),
        };

        if &session.user_pubkey == public_key {
            user_sessions.push(session);
        }
    }

    if user_sessions.is_empty() {
        tracing::warn!("No valid sessions found for pubky-host: {}", public_key);
        return Err(HttpError::forbidden_with_message(
            "No valid session found for user",
        ));
    }

    Ok(user_sessions)
}

/// Get all session secrets from the cookies by iterating and validating.
/// Returns a vector of all valid session secrets found.
pub fn session_secrets_from_cookies(cookies: &Cookies) -> Vec<SessionSecret> {
    let mut secrets = Vec::new();
    for cookie in cookies.list() {
        if let Ok(secret) = SessionSecret::from_str(cookie.value()) {
            secrets.push(secret);
        }
    }
    secrets
}
