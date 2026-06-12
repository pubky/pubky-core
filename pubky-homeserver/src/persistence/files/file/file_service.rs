#[cfg(test)]
use crate::AppContext;
use crate::{
    persistence::{
        files::events::EventsService,
        sql::{
            entry::{EntryEntity, EntryRepository},
            SqlDb, UnifiedExecutor,
        },
    },
    shared::webdav::EntryPath,
    ConfigToml,
};
use bytes::Bytes;
use futures_util::Stream;
#[cfg(test)]
use futures_util::StreamExt;
#[cfg(test)]
use opendal::Buffer;
use std::path::Path;

use super::super::{FileIoError, FileStream, OpendalService, WriteStreamError};

/// The file service creates an abstraction layer over the SqlDb and OpenDAL services.
/// This way, files can be managed in a unified way.
#[derive(Debug, Clone)]
pub struct FileService {
    pub(crate) opendal: OpendalService,
    pub(crate) db: SqlDb,
}

impl FileService {
    pub fn new(opendal_service: OpendalService, db: SqlDb) -> Self {
        Self {
            opendal: opendal_service,
            db,
        }
    }

    pub fn new_from_config(
        config: &ConfigToml,
        data_directory: &Path,
        db: SqlDb,
        events_service: EventsService,
        user_service: crate::services::user_service::UserService,
    ) -> Result<Self, FileIoError> {
        let opendal_service = OpendalService::new_from_config(
            &config.storage,
            data_directory,
            &db,
            events_service,
            user_service,
        )?;
        Ok(Self::new(opendal_service, db))
    }

    /// Get the metadata of a file.
    pub async fn get_info(
        &self,
        path: &EntryPath,
        executor: &mut UnifiedExecutor<'_>,
    ) -> Result<EntryEntity, FileIoError> {
        match EntryRepository::get_by_path(path, executor).await {
            Ok(entry) => Ok(entry),
            Err(sqlx::Error::RowNotFound) => Err(FileIoError::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the content of a file as a stream of bytes.
    /// The stream is chunked.
    /// Errors if the file does not exist.
    pub async fn get_stream(&self, path: &EntryPath) -> Result<FileStream, FileIoError> {
        let stream: FileStream = self.opendal.get_stream(path).await?;
        Ok(stream)
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write_stream(
        &self,
        path: &EntryPath,
        stream: impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
    ) -> Result<EntryEntity, FileIoError> {
        self.opendal.write_stream(path, stream).await?;
        match EntryRepository::get_by_path(path, &mut self.db.pool().into()).await {
            Ok(entry) => Ok(entry),
            Err(sqlx::Error::RowNotFound) => Err(FileIoError::NotFound),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a file.
    pub async fn delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        if !self.opendal.exists(path).await? {
            return Err(FileIoError::NotFound);
        }
        self.opendal.delete(path).await?;
        Ok(())
    }

    /// Delete a file bypassing write-path restrictions.
    /// Used by the admin `/webdav` REST delete route; the `/dav` WebDAV handler
    /// already uses `admin_operator` directly and does not need this.
    pub async fn admin_delete(&self, path: &EntryPath) -> Result<(), FileIoError> {
        if !self.opendal.exists(path).await? {
            return Err(FileIoError::NotFound);
        }
        self.opendal.admin_delete(path).await?;
        Ok(())
    }
}

#[cfg(test)]
impl FileService {
    pub fn new_from_context(context: &AppContext) -> Result<Self, FileIoError> {
        let opendal_service = OpendalService::new(context)?;
        Ok(Self::new(opendal_service, context.sql_db.clone()))
    }

    /// Get the content of a file as bytes.
    /// Errors if the file does not exist.
    pub async fn get(&self, path: &EntryPath) -> Result<Bytes, FileIoError> {
        let mut stream = self.get_stream(path).await?;
        let mut collected_data = Vec::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            collected_data.extend_from_slice(&chunk);
        }

        Ok(Bytes::from(collected_data))
    }

    /// Write a file to the database and storage depending on the selected target location.
    pub async fn write(&self, path: &EntryPath, data: Buffer) -> Result<EntryEntity, FileIoError> {
        let stream = futures_util::stream::iter(vec![Ok(Bytes::from(data.to_vec()))]);
        let entry = self.write_stream(path, stream).await?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        persistence::sql::user::UserRepository, services::user_service::FILE_METADATA_SIZE,
        shared::webdav::WebDavPath,
    };
    use futures_lite::StreamExt;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_write_get_delete_db_and_opendal() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();
        let pubkey = pubky_common::crypto::Keypair::random().public_key();

        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // User should not have any data usage yet
        assert_eq!(user.used_bytes, 0);

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());

        // Test getting a non-existent file
        match file_service.get_stream(&path).await {
            Ok(_) => panic!("Should error for non-existent file"),
            Err(FileIoError::NotFound) => {}
            Err(e) => panic!("Should error for non-existent file: {}", e),
        };

        // Test data
        let test_data = b"Hello, world! This is test data for the get method.";
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);

        file_service.write_stream(&path, stream).await.unwrap();
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            user.used_bytes,
            test_data.len() as u64 + FILE_METADATA_SIZE,
            "Data usage should be the size of the file"
        );

        // Get the file content and verify
        let mut stream = file_service
            .get_stream(&path)
            .await
            .expect("File should exist");
        let mut collected_data = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected_data.extend_from_slice(&chunk);
        }

        assert_eq!(
            collected_data,
            test_data.to_vec(),
            "Content should match original data"
        );

        file_service.delete(&path).await.unwrap();
        let result = file_service.get_stream(&path).await;
        assert!(result.is_err(), "Should error for deleted file");
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            user.used_bytes, 0,
            "Data usage should be 0 after deleting file"
        );

        // Test OpenDal location
        let path = EntryPath::new(
            pubkey.clone(),
            WebDavPath::new("/test_opendal.txt").unwrap(),
        );
        let chunks = vec![Ok(Bytes::from(test_data.as_slice()))];
        let stream = futures_util::stream::iter(chunks);
        file_service.write_stream(&path, stream).await.unwrap();
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            user.used_bytes,
            test_data.len() as u64 + FILE_METADATA_SIZE,
            "Data usage should be the size of the file"
        );

        // Get the file content and verify
        let mut stream = file_service
            .get_stream(&path)
            .await
            .expect("File should exist");
        let mut collected_data = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected_data.extend_from_slice(&chunk);
        }

        assert_eq!(
            collected_data,
            test_data.to_vec(),
            "Content should match original data for OpenDal location"
        );

        // Clean up
        file_service.delete(&path).await.unwrap();
        let result = file_service.get_stream(&path).await;
        assert!(result.is_err(), "Should error for deleted file");
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            user.used_bytes, 0,
            "Data usage should be 0 after deleting file"
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_write_get_basic() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let test_data = b"Hello, world!";
        let buffer = Buffer::from(test_data.as_slice());

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        file_service.write(&path, buffer.clone()).await.unwrap();
        let content = file_service.get(&path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);

        // Test OpenDal
        let opendal_path = EntryPath::new(pubkey, WebDavPath::new("/test_opendal.txt").unwrap());
        file_service.write(&opendal_path, buffer).await.unwrap();
        let content = file_service.get(&opendal_path).await.unwrap();
        assert_eq!(content.as_ref(), test_data);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_data_usage_update_basic() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create_with_quota_mb(&db, &pubkey, 1).await;

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.used_bytes, test_data.len() as u64 + FILE_METADATA_SIZE);

        // Delete the file and check if the data usage is updated correctly.
        file_service.delete(&path).await.unwrap();
        let user = UserRepository::get(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.used_bytes, 0);
    }

    /// Override and existing entry and check if the data usage is updated correctly.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_data_usage_override_existing_entry() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create_with_quota_mb(&db, &pubkey, 1).await;

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        let test_data2 = vec![2u8; 1024];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());

        file_service.write(&path, buffer2).await.unwrap();

        assert_eq!(
            UserRepository::get(&pubkey, &mut db.pool().into())
                .await
                .unwrap()
                .used_bytes,
            test_data2.len() as u64 + FILE_METADATA_SIZE
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_rejects_descendant_write_without_target_artifacts() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path = EntryPath::new(
            pubkey.clone(),
            WebDavPath::new("/pub/app/foo/bar.json").unwrap(),
        );

        file_service
            .write(&exact_path, Buffer::from(vec![1; 10]))
            .await
            .unwrap();
        let err = file_service
            .write(&descendant_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("descendant write should be rejected");

        assert!(matches!(err, FileIoError::PathCollision));
        file_service
            .get_info(&descendant_path, &mut db.pool().into())
            .await
            .expect_err("Rejected descendant should not create metadata");
        file_service
            .get(&descendant_path)
            .await
            .expect_err("Rejected descendant should not create a blob");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_rejects_exact_write_without_target_artifacts() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/app/foo/bar.json").unwrap());

        file_service
            .write(&descendant_path, Buffer::from(vec![1; 10]))
            .await
            .unwrap();
        let err = file_service
            .write(&exact_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("exact-file write should be rejected");

        assert!(matches!(err, FileIoError::PathCollision));
        file_service
            .get_info(&exact_path, &mut db.pool().into())
            .await
            .expect_err("Rejected exact file should not create metadata");
        file_service
            .get(&exact_path)
            .await
            .expect_err("Rejected exact file should not create a blob");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_rejects_descendant_while_exact_write_is_open() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/app/foo/bar.json").unwrap());

        let mut exact_writer = file_service
            .opendal
            .operator
            .writer(exact_path.as_str())
            .await
            .unwrap();
        exact_writer.write(Buffer::from(vec![1; 10])).await.unwrap();

        let err = file_service
            .write(&descendant_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("descendant write should be rejected while exact file is reserved");
        assert!(matches!(err, FileIoError::PathCollision));

        exact_writer.close().await.unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_rejects_exact_while_descendant_write_is_open() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/app/foo/bar.json").unwrap());

        let mut descendant_writer = file_service
            .opendal
            .operator
            .writer(descendant_path.as_str())
            .await
            .unwrap();
        descendant_writer
            .write(Buffer::from(vec![1; 10]))
            .await
            .unwrap();

        let err = file_service
            .write(&exact_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("exact-file write should be rejected while descendant is reserved");
        assert!(matches!(err, FileIoError::PathCollision));

        descendant_writer.close().await.unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_abort_releases_reservation() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/app/foo/bar.json").unwrap());

        let mut exact_writer = file_service
            .opendal
            .operator
            .writer(exact_path.as_str())
            .await
            .unwrap();
        exact_writer.write(Buffer::from(vec![1; 10])).await.unwrap();

        let err = file_service
            .write(&descendant_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("descendant write should be rejected while exact file is reserved");
        assert!(matches!(err, FileIoError::PathCollision));

        exact_writer.abort().await.unwrap();
        file_service
            .write(&descendant_path, Buffer::from(vec![3; 10]))
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_drop_releases_reservation() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path = EntryPath::new(pubkey.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path =
            EntryPath::new(pubkey, WebDavPath::new("/pub/app/foo/bar.json").unwrap());

        let mut exact_writer = file_service
            .opendal
            .operator
            .writer(exact_path.as_str())
            .await
            .unwrap();
        exact_writer.write(Buffer::from(vec![1; 10])).await.unwrap();

        let err = file_service
            .write(&descendant_path, Buffer::from(vec![2; 10]))
            .await
            .expect_err("descendant write should be rejected while exact file is reserved");
        assert!(matches!(err, FileIoError::PathCollision));

        drop(exact_writer);

        for _ in 0..20 {
            match file_service
                .write(&descendant_path, Buffer::from(vec![3; 10]))
                .await
            {
                Ok(_) => return,
                Err(FileIoError::PathCollision) => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => panic!("unexpected write error after dropped writer: {e}"),
            }
        }

        panic!("dropped writer should release reservation promptly");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_does_not_serialize_cross_user_writes() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey_a = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey_a, &mut db.pool().into())
            .await
            .unwrap();
        let pubkey_b = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey_b, &mut db.pool().into())
            .await
            .unwrap();

        let path_a = EntryPath::new(pubkey_a, WebDavPath::new("/pub/app/foo").unwrap());
        let path_b = EntryPath::new(pubkey_b, WebDavPath::new("/pub/app/foo").unwrap());

        let mut writer_a = file_service
            .opendal
            .operator
            .writer(path_a.as_str())
            .await
            .unwrap();
        writer_a.write(Buffer::from(vec![1; 10])).await.unwrap();

        let concurrent_service = file_service.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let concurrent_write = tokio::spawn(async move {
            let _ = started_tx.send(());
            concurrent_service
                .write(&path_b, Buffer::from(vec![2; 10]))
                .await
        });
        started_rx.await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), concurrent_write)
            .await
            .expect("cross-user write should not wait for the first user's open writer")
            .unwrap()
            .unwrap();

        writer_a.close().await.unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_path_collision_same_user_burst_does_not_block_unrelated_user() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey_a = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey_a, &mut db.pool().into())
            .await
            .unwrap();
        let pubkey_b = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create(&pubkey_b, &mut db.pool().into())
            .await
            .unwrap();

        let exact_path_a =
            EntryPath::new(pubkey_a.clone(), WebDavPath::new("/pub/app/foo").unwrap());
        let descendant_path_a =
            EntryPath::new(pubkey_a, WebDavPath::new("/pub/app/foo/bar.json").unwrap());
        let path_b = EntryPath::new(pubkey_b, WebDavPath::new("/pub/app/foo").unwrap());

        let mut writer_a = file_service
            .opendal
            .operator
            .writer(exact_path_a.as_str())
            .await
            .unwrap();
        writer_a.write(Buffer::from(vec![1; 10])).await.unwrap();

        let mut conflicting_writes = Vec::new();
        for _ in 0..20 {
            let concurrent_service = file_service.clone();
            let path = descendant_path_a.clone();
            conflicting_writes.push(tokio::spawn(async move {
                concurrent_service
                    .write(&path, Buffer::from(vec![2; 10]))
                    .await
            }));
        }

        tokio::time::timeout(
            Duration::from_secs(2),
            file_service.write(&path_b, Buffer::from(vec![3; 10])),
        )
        .await
        .expect("unrelated user write should not wait behind same-user conflicts")
        .unwrap();

        for write in conflicting_writes {
            let err = write
                .await
                .unwrap()
                .expect_err("same-user conflicting write should fail fast");
            assert!(matches!(err, FileIoError::PathCollision));
        }

        writer_a.abort().await.unwrap();
    }

    /// Write a file that is exactly at the quota and check if the data usage is updated correctly.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_data_usage_exactly_to_quota() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create_with_quota_mb(&db, &pubkey, 1).await;

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024 - FILE_METADATA_SIZE as usize];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        assert_eq!(
            UserRepository::get(&pubkey, &mut db.pool().into())
                .await
                .unwrap()
                .used_bytes,
            test_data.len() as u64 + FILE_METADATA_SIZE
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_data_usage_above_quota() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create_with_quota_mb(&db, &pubkey, 1).await;

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        let test_data = vec![1u8; 1024 * 1024 + 1];
        let buffer = Buffer::from(test_data.clone());

        match file_service.write(&path, buffer).await {
            Ok(_) => panic!("Should error for file above quota"),
            Err(FileIoError::DiskSpaceQuotaExceeded) => {} // All good
            Err(e) => {
                panic!("Should error for file above quota: {:?}", e);
            }
        }

        assert_eq!(
            UserRepository::get(&pubkey, &mut db.pool().into())
                .await
                .unwrap()
                .used_bytes,
            0
        );
    }

    /// Override and existing entry and check if the data usage is updated correctly.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_data_usage_override_existing_above_quota() {
        let context = AppContext::test().await;
        let file_service = FileService::new_from_context(&context).unwrap();
        let db = context.sql_db.clone();

        let pubkey = pubky_common::crypto::Keypair::random().public_key();
        UserRepository::create_with_quota_mb(&db, &pubkey, 1).await;

        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());
        let test_data = vec![1u8; 1024];
        let buffer = Buffer::from(test_data.clone());

        file_service.write(&path, buffer).await.unwrap();

        let test_data2 = vec![2u8; 1024 * 1024 + 1];
        let buffer2 = Buffer::from(test_data2.clone());
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test_file.txt").unwrap());

        match file_service.write(&path, buffer2).await {
            Ok(_) => panic!("Should error for file above quota"),
            Err(FileIoError::DiskSpaceQuotaExceeded) => {} // All good
            Err(e) => {
                panic!("Should error for file above quota: {:?}", e);
            }
        }

        assert_eq!(
            UserRepository::get(&pubkey, &mut db.pool().into())
                .await
                .unwrap()
                .used_bytes,
            test_data.len() as u64 + FILE_METADATA_SIZE
        );
    }
}
