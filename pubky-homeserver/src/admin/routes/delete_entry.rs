use super::super::app_state::AppState;
use crate::{persistence::lmdb::tables::entries::EntryPath, shared::{HttpError, HttpResult, WebDavPathAxum, Z32Pubkey}};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

/// Delete a single entry from the database.
pub async fn delete_entry(
    State(mut state): State<AppState>,
    Path((pubkey, path)): Path<(Z32Pubkey, WebDavPathAxum)>,
) -> HttpResult<impl IntoResponse> {

    let entry_path = EntryPath::new(pubkey.0, path.0);
    let deleted = state.db.delete_entry2(&entry_path).await?;
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
    use crate::persistence::lmdb::{tables::entries::InDbTempFile, LmDB};
    use crate::persistence::lmdb::tables::entries::{EntryPath};
    use crate::shared::WebDavPath;
    use axum::{routing::delete, Router};
    use pkarr::Keypair;

    async fn write_test_file(db: &mut LmDB, entry_path: &EntryPath) {
        let file = InDbTempFile::zeros(10).await.unwrap();
        let _entry = db.write_entry2(&entry_path, &file).await.unwrap();
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
            .route("/webdav/{pubkey}/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Write a test file
        let webdav_path = WebDavPath::new(format!("/pub/{}", file_path).as_str()).unwrap();
        let entry_path = EntryPath::new(pubkey.clone(), webdav_path);

        write_test_file(&mut db, &entry_path).await;

        // Delete the file
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/{}{}", pubkey, entry_path.path().as_str()).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::NO_CONTENT);

        // Check that the file is deleted
        let entry = db.get_entry2(&entry_path).unwrap();
        assert!(entry.is_none(), "Entry should be deleted");

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
        let app_state = AppState::new(LmDB::test());
        let router = Router::new()
            .route("/webdav/{pubkey}/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Delete the file
        let url = format!("/webdav/{}/pub/{}", pubkey, file_path);
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(url.as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_invalid_pubkey() {
        // Set everything up
        let db = LmDB::test();
        let app_state = AppState::new(db.clone());
        let router = Router::new()
            .route("/webdav/{pubkey}/{*path}", delete(delete_entry))
            .with_state(app_state);

        // Delete with invalid pubkey
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .delete(format!("/webdav/1234/pub/test.txt").as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    }
}
