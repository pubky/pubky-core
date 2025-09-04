use axum::response::IntoResponse;

pub async fn handler() -> Result<impl IntoResponse, String> {
    Ok("Pubky Homeserver")
}
