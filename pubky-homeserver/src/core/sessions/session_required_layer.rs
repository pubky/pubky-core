use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::future::BoxFuture;
use pkarr::PublicKey;
use pubky_common::session::Session;
use reqwest::Method;
use std::{convert::Infallible, task::Poll};
use tower::{Layer, Service};
use tower_cookies::Cookies;

use crate::core::{
    error::{Error, Result},
    AppState,
};
use crate::persistence::lmdb::tables::sessions::SessionId;

/// A Tower Layer that makes sure the request has a valid session.
/// 
/// The session is inserted into the request extensions.
/// 
/// You can access the session in the request extensions using the `UserSession` extractor.
/// 
/// Returns a 401 Unauthorized if the request does not have a valid session.
#[derive(Debug, Clone)]
pub struct SessionRequiredLayer {
    state: AppState,
}

impl SessionRequiredLayer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl<S> Layer<S> for SessionRequiredLayer {
    type Service = SessionRequiredMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SessionRequiredMiddleware {
            inner,
            state: self.state.clone(),
        }
    }
}

/// Middleware that performs session checks..
#[derive(Debug, Clone)]
pub struct SessionRequiredMiddleware<S> {
    inner: S,
    state: AppState,
}

impl<S> Service<Request<Body>> for SessionRequiredMiddleware<S>
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
            let (session, id) = match authorize(&state, &req) {
                Ok(session) => session,
                Err(e) => {
                    if req.uri().path() == "/session" && req.method() == Method::GET {
                        // To guarantee backward compatibility, with the old pubky client we return a 404 Not Found
                        // For the session endpoint
                        // Sev 7th of May 2025
                        return Ok(Error::with_status(StatusCode::NOT_FOUND).into_response());
                    }
                    return Ok(e.into_response())
                },
            };

            req.extensions_mut().insert(UserSession::new(session, id));

            // Proceed to the inner service
            inner.call(req).await.map_err(|_| unreachable!())
        })
    }
}

/// Authorize write (PUT or DELETE) for Public paths.
fn authorize(
    state: &AppState,
    req: &Request<Body>,
) -> Result<(Session, SessionId)> {

    let unauthorized_err = Err(Error::with_status(StatusCode::UNAUTHORIZED));

    let cookies = match req.extensions().get::<Cookies>() {
        Some(cookies) => cookies,
        None => {
            // No cookies means no session
            return unauthorized_err;
        }
    };

    let (session, id) = match state
        .session_manager
        .extract_session_from_cookies(cookies)
    {
        Some(session) => session,
        None => {
            // Failed to extract session ID from cookies
            return unauthorized_err;
        }
    };

    // User still active check
    let user = match state.db.get_user(session.pubky(), &mut state.db.env.read_txn()?) {
        Ok(user) => user,
        Err(_) => {
            // User not found
            return unauthorized_err;
        },
    };
    if user.disabled {
        return unauthorized_err;
    }

    Ok((session, id))
}


/// Axum extractor for the user session.
/// Use this to access the session in the request extensions.
#[derive(Debug, Clone)]
pub struct UserSession{
    pub(crate) id: SessionId,
    pub(crate) session: Session,
}

impl UserSession {
    pub fn new(session: Session, id: SessionId) -> Self {
        Self { session, id }
    }
}

impl<S> FromRequestParts<S> for UserSession
where
    S: Sync + Send,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session = parts
            .extensions
            .get::<UserSession>()
            .cloned()
            .ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Can't extract UserSession. Is `SessionRequiredLayer` enabled?",
            ))
            .map_err(|e| e.into_response())?;

        Ok(session)
    }
}