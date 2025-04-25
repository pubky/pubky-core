use super::super::app_state::AppState;
use crate::shared::{HttpError, HttpResult, Z32Pubkey};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

/// Delete a single entry from the database.
pub async fn delete_entry(
    State(mut state): State<AppState>,
    Path((pubkey, path)): Path<(Z32Pubkey, String)>,
) -> HttpResult<impl IntoResponse> {
    let full_path = format!("/pub/{}", path);
    let deleted = state.db.delete_entry(&pubkey.0, &full_path)?;
    if deleted {
        Ok((StatusCode::NO_CONTENT, ()))
    } else {
        Err(HttpError::new(
            StatusCode::NOT_FOUND,
            Some("Entry not found"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::app_state::AppState;
    use super::*;
    use crate::persistence::lmdb::LmDB;
    use axum::{routing::delete, Router};
    use pkarr::{Keypair, PublicKey};
    use std::io::Write;

    async fn write_test_file(db: &mut LmDB, pubkey: &PublicKey, path: &str) {
        let mut entry_writer = db.write_entry(pubkey, path).unwrap();
        let content = b"Hello, world!";
        entry_writer.write_all(content).unwrap();
        let _entry = entry_writer.commit().unwrap();
    }

    #[tokio::test]
    async fn test_delete_entry() {
        // Set everything up
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubkey = keypair.public_key();
        let file_path = "my_file.txt";
        let mut db = LmDB::test();
        let app_state = AppState::new(db.clone());
        let router = Router::new()
            .route("/webdav/{pubkey}/pub/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Write a test file
        let entry_path = format!("/pub/{}", file_path);
        write_test_file(&mut db, &pubkey, &entry_path).await;

        // Delete the file
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/{}/pub/{}", pubkey, file_path).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the file is deleted
        let rtx = db.env.read_txn().unwrap();
        let entry = db.get_entry(&rtx, &pubkey, &file_path).unwrap();
        assert!(entry.is_none(), "Entry should be deleted");
    }

    #[tokio::test]
    async fn test_file_not_found() {
        // Set everything up
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubkey = keypair.public_key();
        let file_path = "my_file.txt";
        let app_state = AppState::new(LmDB::test());
        let router = Router::new()
            .route("/webdav/{pubkey}/pub/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Delete the file
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/{}/pub/{}", pubkey, file_path).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_invalid_pubkey() {
        // Set everything up
        let db = LmDB::test();
        let app_state = AppState::new(db.clone());
        let router = Router::new()
            .route("/webdav/{pubkey}/pub/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Delete with invalid pubkey
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/1234/pub/test.txt").as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    }
}
