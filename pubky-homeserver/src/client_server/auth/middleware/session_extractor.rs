//! Axum extractor for [`AuthSession`].
//!
//! Pulls the session inserted by the authentication middleware from request
//! extensions. Separated from the [`AuthSession`] definition to keep the
//! domain type free of framework imports.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;

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
