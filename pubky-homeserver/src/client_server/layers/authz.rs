use crate::client_server::{extractors::PubkyHost, AppState};
use crate::persistence::sql::session::{SessionRepository, SessionSecret};
use crate::shared::{HttpError, HttpResult};
use axum::http::{HeaderMap, Method};
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::engine::general_purpose::STANDARD as Base64;
use base64::Engine;
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

/// A Tower Layer to handle authorization for write operations.
#[derive(Clone)]
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
#[derive(Clone)]
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
            let public_key = match req.extensions().get::<PubkyHost>() {
                Some(pk) => pk.public_key(),
                None => {
                    tracing::warn!("Pubky Host is missing in request. Authorization failed.");
                    return Ok(HttpError::new_with_message(
                        StatusCode::NOT_FOUND,
                        "Pubky Host is missing",
                    )
                    .into_response());
                }
            };

            let path = req.uri().path();
            let headers = req.headers();
            let cookies = req.extensions().get::<Cookies>();
            // Authorize the request
            if let Err(e) =
                authorize(&state, req.method(), cookies, public_key, path, headers).await
            {
                return Ok(e.into_response());
            };

            // If authorized, proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize write (PUT or DELETE) for Public paths.
async fn authorize(
    state: &AppState,
    method: &Method,
    cookies: Option<&Cookies>,
    public_key: &PublicKey,
    path: &str,
    headers: &HeaderMap,
) -> HttpResult<()> {
    if path == "/session" {
        // Checking (or deleting) one's session is ok for everyone
        return Ok(());
    } else if path.starts_with("/pub/") {
        if method == Method::GET || method == Method::HEAD {
            return Ok(());
        }
    } else if path.starts_with("/dav/") {
        // XXX: at least for now
        // if method == Method::GET {
        //     return Ok(());
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

    let session_secret = if path.starts_with("/dav") {
        match session_secret_from_header(headers, public_key) {
            Some(session_secret) => session_secret,
            None => {
                tracing::warn!(
                    "No session secret found in Authorization header for pubky-host: {}",
                    public_key
                );
                return Err(HttpError::unauthorized_with_message(
                    "No session secret found in Authorization header",
                ));
            }
        }
    } else {
        match session_secret_from_cookies(cookies, public_key) {
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
        Ok(())
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

pub fn session_secret_from_cookies(
    cookies: Option<&Cookies>,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    cookies?
        .get(&public_key.to_string())
        .and_then(|cookie| SessionSecret::new(cookie.value().to_string()).ok())
}

fn session_secret_from_header(
    headers: &HeaderMap,
    public_key: &PublicKey,
) -> Option<SessionSecret> {
    let auth_header = headers.get("Authorization")?;
    let auth_str = auth_header.to_str().ok()?;
    let base64_encoded = auth_str.strip_prefix("Basic ")?;
    let decoded = Base64.decode(base64_encoded.trim()).ok()?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    let (key, secret) = decoded_str.split_once(':')?;

    if key == public_key.to_string() {
        SessionSecret::new(secret.to_string()).ok()
    } else {
        None
    }
}
