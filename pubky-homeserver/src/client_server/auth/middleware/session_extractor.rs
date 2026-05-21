//! Axum extractors for [`AuthSession`].
//!
//! Pulls the session inserted by the authentication middleware from request
//! extensions. Separated from the [`AuthSession`] definition to keep the
//! domain type free of framework imports.
//!
//! Strict handlers take `AuthSession` and get a 401 if the middleware didn't
//! insert one. Lenient handlers (e.g. signout) take `Option<AuthSession>` and
//! receive `None` instead.

use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::convert::Infallible;

use crate::client_server::auth::AuthSession;

impl<S> FromRequestParts<S> for AuthSession
where
    S: Send + Sync,
{
    type Rejection = axum::response::Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthSession>()
            .cloned()
            .ok_or((StatusCode::UNAUTHORIZED, "No authenticated session found"))
            .map_err(|e| e.into_response())
    }
}

impl<S> OptionalFromRequestParts<S> for AuthSession
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<AuthSession>().cloned())
    }
}
