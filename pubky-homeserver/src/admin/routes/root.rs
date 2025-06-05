use axum::{http::StatusCode, response::IntoResponse};

use crate::shared::HttpResult;

pub async fn root() -> HttpResult<impl IntoResponse> {
    Ok((StatusCode::OK, "Homeserver - Admin Endpoint"))
}
