use axum::response::IntoResponse;

use tracing::debug;

use crate::extractors::Pubky;

pub async fn put(pubky: Pubky) -> Result<impl IntoResponse, String> {
    debug!(pubky=?pubky.public_key());

    Ok("Pubky drive...".to_string())
}
