use crate::shared::{HttpError, HttpResult, Z32Pubkey};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use super::super::app_state::AppState;


/// Delete a single entry from the database.
/// 
/// # Errors
/// 
/// - `400` if the pubkey is invalid.
/// - `404` if the entry does not exist.
/// 
pub async fn disable_tenant(
    State(mut state): State<AppState>,
    Path(pubkey): Path<Z32Pubkey>,
) -> HttpResult<impl IntoResponse> {

    Ok((StatusCode::OK, "Ok"))
}


#[cfg(test)]
mod tests {
    use std::io::Write;
    use crate::persistence::lmdb::LmDB;
    use axum::routing::post;
    use axum::Router;
    use pkarr::{Keypair, PublicKey};
    use super::super::super::app_state::AppState;
    use super::*;

    async fn write_test_file(db: &mut LmDB, pubkey: &PublicKey, path: &str) {
        let mut entry_writer = db.write_entry(pubkey, path).unwrap();
        let content = b"Hello, world!";
        entry_writer.write_all(content).unwrap();
        let _entry = entry_writer.commit().unwrap();
    }

    #[tokio::test]
    async fn test_delete_entry() {
        // Set everything up
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();
        let mut db = LmDB::test();
        
        let app_state = AppState::new(db.clone());
        let router = Router::new()
            .route("/tenants/{pubkey}/disable", post(disable_tenant))
            .with_state(app_state);


        // Delete the file
        let _server = axum_test::TestServer::new(router).unwrap();
        // let response = server.delete(format!("/drive/{}/pub/{}", pubkey, file_path).as_str()).await;
        // assert_eq!(response.status_code(), StatusCode::OK);

    }
}
