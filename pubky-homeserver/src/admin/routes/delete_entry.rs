use crate::core::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use super::super::app_state::AppState;

pub async fn delete_entry(
    State(mut state): State<AppState>,
    Path(path): Path<String>,
) -> Result<impl IntoResponse> {
    println!("Path: {}", path);
    Ok((StatusCode::OK, "Ok"))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use crate::persistence::lmdb::LmDB;
    use axum::{routing::delete, Router};
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
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubkey = keypair.public_key();
        let file_path = "/my_file.txt";
        let mut db = LmDB::test();
        let app_state = AppState::new(db.clone());
        let router = Router::new()
            .route("/drive/pub/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Write a test file
        write_test_file(&mut db, &pubkey, &file_path).await;

        // Delete the file
        let path = format!("{}{}", pubkey, file_path);
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server.delete(format!("/drive/pub/{}", path).as_str()).await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the file is deleted
        let rtx = db.env.read_txn().unwrap();
        let entry = db.get_entry(&rtx, &pubkey, &file_path).unwrap();
        assert!(entry.is_none());
    }
    
}
