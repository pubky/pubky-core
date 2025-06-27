use super::super::app_state::AppState;
use crate::shared::{webdav::EntryPathPub, HttpResult};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

/// Delete a single entry from the database.
pub async fn delete_entry(
    State(state): State<AppState>,
    Path(entry_path): Path<EntryPathPub>,
) -> HttpResult<impl IntoResponse> {
    state.file_service.delete(entry_path.inner()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::super::super::app_state::AppState;
    use super::*;
    use crate::persistence::files::{FileIoError, FileService};
    use crate::persistence::lmdb::{ LmDB};
    use crate::shared::webdav::{EntryPath, WebDavPath};
    use axum::{routing::delete, Router};
    use opendal::Buffer;
    use pkarr::Keypair;

    async fn write_test_file(file_service: &FileService, entry_path: &EntryPath) {
        let buffer = Buffer::from(vec![0; 10]);
        let _entry = file_service.write(entry_path, buffer).await.unwrap();
    }

    #[tokio::test]
    async fn test_delete_entry() {
        // Set everything up
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubkey = keypair.public_key();
        let file_path = "my_file.txt";
        let mut db = LmDB::test();
        let file_service = FileService::test(db.clone());
        let app_state = AppState::new(db.clone(), file_service.clone());
        let router = Router::new()
            .route("/webdav/{*entry_path}", delete(delete_entry))
            .with_state(app_state);

        // Write a test file
        let webdav_path = WebDavPath::new(format!("/pub/{}", file_path).as_str()).unwrap();
        db.create_user(&pubkey).unwrap();
        let entry_path = EntryPath::new(pubkey.clone(), webdav_path);

        write_test_file(&file_service, &entry_path).await;

        // Delete the file
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/{}{}", pubkey, entry_path.path().as_str()).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::NO_CONTENT);

        // Check that the file is deleted
        match db.get_entry(&entry_path) {
            Ok(entry) => {
                panic!("Entry should be deleted: {:?}", entry);
            }
            Err(FileIoError::NotFound) => {
                // Ok
            }
            Err(e) => {
                panic!("Error getting entry: {}", e);
            }
        }

        let events = db.list_events(None, None).unwrap();
        assert_eq!(
            events.len(),
            3,
            "One PUT and one DEL event should be created. Last entry is the cursor."
        );
        assert!(events[0].contains("PUT"));
        assert!(events[1].contains("DEL"));
    }

    #[tokio::test]
    async fn test_file_not_found() {
        // Set everything up
        let keypair = Keypair::from_secret_key(&[0; 32]);
        let pubkey = keypair.public_key();
        let file_path = "my_file.txt";
        let db = LmDB::test();
        let app_state = AppState::new(db.clone(), FileService::test(db.clone()));
        let router = Router::new()
            .route("/webdav/{*entry_path}", delete(delete_entry))
            .with_state(app_state);

        // Delete the file
        let url = format!("/webdav/{}/pub/{}", pubkey, file_path);
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server.delete(url.as_str()).await;
        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_invalid_pubkey() {
        // Set everything up
        let db = LmDB::test();
        let app_state = AppState::new(db.clone(), FileService::test(db.clone()));
        let router = Router::new()
            .route("/webdav/{*entry_path}", delete(delete_entry))
            .with_state(app_state);

        // Delete with invalid pubkey
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete("/webdav/1234/pub/test.txt".to_string().as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    }
}
