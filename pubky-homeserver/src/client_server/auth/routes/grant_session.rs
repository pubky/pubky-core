//! Grant-based session creation handler.
//!
//! Accepts a Grant JWS + PoP proof, verifies both, and returns an Access JWT.
//! This is the grant-based alternative to the deprecated cookie-based `signin()`.

use axum::{extract::State, response::IntoResponse, Json};
use serde::Deserialize;

use crate::{
    client_server::{auth::crypto::jws_crypto::JwsCompact, AppState},
    shared::HttpResult,
};

/// JSON request body for grant-based session creation.
#[derive(Deserialize)]
pub struct CreateGrantSessionRequest {
    /// Grant JWS (user-signed).
    pub grant: JwsCompact,
    /// PoP proof JWS (client-signed).
    pub pop: JwsCompact,
}

/// Handle `POST /session` with JSON body (grant-based auth).
pub async fn create_grant_session(
    State(state): State<AppState>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let response = state.auth_service.create_grant_session(request).await?;
    Ok(Json(response))
}
