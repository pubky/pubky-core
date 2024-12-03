use axum::response::IntoResponse;

pub async fn handler() -> Result<impl IntoResponse, String> {
    Ok("This a Pubky homeserver.".to_string())
}
