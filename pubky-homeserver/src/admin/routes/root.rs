
use axum::{http::StatusCode, response::IntoResponse};
use crate::core::Result;


pub async fn root() -> Result<impl IntoResponse> {
    Ok((StatusCode::OK, "Homeserver - Admin Endpoint"))
}

