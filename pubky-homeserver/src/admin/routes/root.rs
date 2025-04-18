use crate::core::Result;
use axum::{http::StatusCode, response::IntoResponse};

pub async fn root() -> Result<impl IntoResponse> {
    Ok((StatusCode::OK, "Homeserver - Admin Endpoint"))
}
