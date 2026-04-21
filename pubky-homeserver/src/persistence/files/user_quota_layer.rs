use std::collections::HashMap;
use std::sync::Arc;

use pubky_common::crypto::PublicKey;

use crate::persistence::files::utils::ensure_valid_path;
use crate::persistence::sql::uexecutor;
use crate::services::user_service::{UserService, FILE_METADATA_SIZE};
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

/// The user quota layer wraps the operator and updates the user quota when a file is written or deleted.
/// It is used to limit the amount of data that a user can store in the homeserver.
/// It will also enforce that only paths in the form of {pubkey}/{path} are allowed.
///
/// The per-user storage quota is read from the `quota_storage_mb` column on
/// the user row in the database. `Default` resolves to `default_storage_quota_mb`
/// (from config), `Unlimited` means no limit, and `Value(n)` means n MB.
#[derive(Clone)]
pub struct UserQuotaLayer {
    user_service: UserService,
}

impl UserQuotaLayer {
    pub fn new(user_service: UserService) -> Self {
        Self { user_service }
    }
}

impl<A: Access> Layer<A> for UserQuotaLayer {
    type LayeredAccess = UserQuotaAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        UserQuotaAccessor {
            inner: Arc::new(inner),
            user_service: self.user_service.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserQuotaAccessor<A: Access> {
    inner: Arc<A>,
    user_service: UserService,
}

impl<A: Access> LayeredAccess for UserQuotaAccessor<A> {
    type Inner = A;
    type Reader = A::Reader;
    type Writer = WriterWrapper<A::Writer, A>;
    type Lister = A::Lister;
    type Deleter = DeleterWrapper<A::Deleter, A>;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    async fn create_dir(&self, path: &str, args: OpCreateDir) -> Result<RpCreateDir> {
        let entry_path = ensure_valid_path(path)?;
        self.inner.create_dir(entry_path.as_str(), args).await
    }

    async fn read(&self, path: &str, args: OpRead) -> Result<(RpRead, Self::Reader)> {
        self.inner.read(path, args).await
    }

    async fn write(&self, path: &str, args: OpWrite) -> Result<(RpWrite, Self::Writer)> {
        let entry_path = ensure_valid_path(path)?;
        let canonical_path = entry_path.to_string();
        let (rp, writer) = self.inner.write(&canonical_path, args).await?;
        Ok((
            rp,
            WriterWrapper {
                inner: writer,
                user_service: self.user_service.clone(),
                bytes_count: 0,
                entry_path,
                inner_accessor: self.inner.clone(),
            },
        ))
    }

    async fn copy(&self, from: &str, to: &str, args: OpCopy) -> Result<RpCopy> {
        let from = ensure_valid_path(from)?;
        let to = ensure_valid_path(to)?;
        self.inner.copy(from.as_str(), to.as_str(), args).await
    }

    async fn rename(&self, from: &str, to: &str, args: OpRename) -> Result<RpRename> {
        let from = ensure_valid_path(from)?;
        let to = ensure_valid_path(to)?;
        self.inner.rename(from.as_str(), to.as_str(), args).await
    }

    async fn stat(&self, path: &str, args: OpStat) -> Result<RpStat> {
        self.inner.stat(path, args).await
    }

    async fn delete(&self) -> Result<(RpDelete, Self::Deleter)> {
        let (rp, deleter) = self.inner.delete().await?;
        Ok((
            rp,
            DeleterWrapper {
                inner: deleter,
                user_service: self.user_service.clone(),
                inner_accessor: self.inner.clone(),
                path_queue: Vec::new(),
            },
        ))
    }

    async fn list(&self, path: &str, args: OpList) -> Result<(RpList, Self::Lister)> {
        self.inner.list(path, args).await
    }

    async fn presign(&self, path: &str, args: OpPresign) -> Result<RpPresign> {
        let entry_path = ensure_valid_path(path)?;
        self.inner.presign(entry_path.as_str(), args).await
    }
}

/// Wrapper around the writer that updates the user quota when the file is closed.
pub struct WriterWrapper<R, A: Access> {
    inner: R,
    user_service: UserService,
    bytes_count: u64,
    entry_path: EntryPath,
    inner_accessor: Arc<A>,
}

impl<R, A: Access> WriterWrapper<R, A> {
    async fn get_current_file_size(&self) -> Result<(u64, bool), opendal::Error> {
        let stats = match self
            .inner_accessor
            .stat(self.entry_path.to_string().as_str(), OpStat::default())
            .await
        {
            Ok(stats) => stats,
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                // If the file does not exist, we assume it was deleted
                // and we don't count it against the user quota
                return Ok((0, false));
            }
            Err(e) => {
                return Err(e);
            }
        };
        let file_size = stats.into_metadata().content_length();
        Ok((file_size, true))
    }
}

impl<R: oio::Write, A: Access> oio::Write for WriterWrapper<R, A> {
    async fn write(&mut self, bs: opendal::Buffer) -> Result<()> {
        // Count bytes that are written to the file.
        self.bytes_count += bs.len() as u64;
        self.inner.write(bs).await
    }

    async fn abort(&mut self) -> Result<()> {
        self.inner.abort().await
    }

    async fn close(&mut self) -> Result<opendal::Metadata> {
        let mut tx = self
            .user_service
            .pool()
            .begin()
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        // Lock the user row to serialize concurrent writes and prevent quota bypass.
        let mut user = self
            .user_service
            .get_for_update(self.entry_path.pubkey(), uexecutor!(tx))
            .await
            .map_err(|e| {
                opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    format!("Failed to get user {}: {}", self.entry_path.pubkey(), e),
                )
            })?;

        let (current_file_size, file_already_exists) = self.get_current_file_size().await?;
        let bytes_delta = if file_already_exists {
            self.bytes_count as i64 - current_file_size as i64
        } else {
            self.bytes_count as i64 - current_file_size as i64 + FILE_METADATA_SIZE as i64
        };

        if self
            .user_service
            .would_exceed_storage_quota(&user, bytes_delta)
        {
            return Err(opendal::Error::new(
                opendal::ErrorKind::RateLimited,
                "User quota exceeded",
            ));
        }

        let metadata = self.inner.close().await?;
        user.used_bytes = user.used_bytes.saturating_add_signed(bytes_delta);
        self.user_service
            .update(&user, uexecutor!(tx))
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        Ok(metadata)
    }
}

/// Helper struct to store the path and the bytes count of a path.
struct DeletePath {
    entry_path: EntryPath,
    /// The size of the file.
    /// If the file does not exist, this is None.
    bytes_count: Option<u64>,
    /// Whether the file exists.
    exists: Option<bool>,
}

impl DeletePath {
    fn new(path: &str) -> anyhow::Result<Self> {
        let entry_path = ensure_valid_path(path)?;
        Ok(Self {
            entry_path,
            bytes_count: None,
            exists: None,
        })
    }

    /// Pull the bytes count of the path.
    pub async fn pull_bytes_count<A: Access>(&mut self, operator: &A) -> Result<()> {
        if self.bytes_count.is_some() {
            // Already got the bytes count
            return Ok(());
        }
        let size = match operator
            .stat(self.entry_path.as_str(), OpStat::default())
            .await
        {
            Ok(stats) => stats.into_metadata().content_length(),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                // If the file does not exist, we assume it was deleted
                // and we don't count it against the user quota
                self.exists = Some(false);
                return Ok(());
            }
            Err(e) => {
                return Err(e);
            }
        };
        self.bytes_count = Some(size);
        self.exists = Some(true);
        Ok(())
    }
}

/// This wrapper is used to delete paths in a queue
/// and update the user quota.
/// Depending on the service backend, each file is deleted in a separate request (filesystem, inmemory)
/// or batched (GCS does 100 paths per batch).
pub struct DeleterWrapper<R, A: Access> {
    inner: R,
    user_service: UserService,
    inner_accessor: Arc<A>,
    path_queue: Vec<DeletePath>,
}

impl<R, A: Access> DeleterWrapper<R, A> {
    async fn update_user_quota(&self, deleted_paths: Vec<DeletePath>) -> Result<()> {
        // Group deleted paths by user pubkey
        let mut user_paths: HashMap<PublicKey, Vec<DeletePath>> = HashMap::new();
        for path in deleted_paths {
            user_paths
                .entry(path.entry_path.pubkey().clone())
                .or_default()
                .push(path);
        }

        for (user_pubkey, paths) in user_paths {
            let mut tx =
                self.user_service.pool().begin().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
            let mut user = match self
                .user_service
                .get_for_update(&user_pubkey, uexecutor!(tx))
                .await
            {
                Ok(user) => user,
                Err(sqlx::Error::RowNotFound) => {
                    // User does not exist in the database, so we don't update the quota.
                    // This can happen if the user was deleted before the file was deleted.
                    // Shouldn't happen but we still handle it.
                    continue;
                }
                Err(e) => {
                    return Err(opendal::Error::new(
                        opendal::ErrorKind::Unexpected,
                        e.to_string(),
                    ));
                }
            };

            let total_bytes: u64 = paths.iter().filter_map(|p| p.bytes_count).sum();
            let files_deleted_count =
                paths.iter().filter(|p| p.exists.unwrap_or(false)).count() as u64;
            let bytes_delta = (total_bytes + files_deleted_count * FILE_METADATA_SIZE) as i64;

            user.used_bytes = user.used_bytes.saturating_add_signed(-bytes_delta);
            self.user_service
                .update(&user, uexecutor!(tx))
                .await
                .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
            tx.commit()
                .await
                .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        }

        Ok(())
    }
}

impl<R: oio::Delete, A: Access> oio::Delete for DeleterWrapper<R, A> {
    async fn flush(&mut self) -> Result<usize> {
        // Get the file size of all paths in the queue
        for path in self.path_queue.iter_mut() {
            path.pull_bytes_count(&self.inner_accessor).await?;
        }

        // Delete the paths and get the number of deleted files
        let deleted_files_count = self.inner.flush().await?;
        // // Get the number of bytes that were deleted
        // let deleted_bytes_count = bytes_list.iter().take(deleted_files_count).sum::<u64>();
        let deleted_paths = self
            .path_queue
            .drain(0..deleted_files_count)
            .collect::<Vec<_>>();
        self.update_user_quota(deleted_paths).await?;
        Ok(deleted_files_count)
    }

    fn delete(&mut self, path: &str, args: OpDelete) -> Result<()> {
        // Add the path to the delete queue.
        let helper = match DeletePath::new(path) {
            Ok(helper) => helper,
            Err(e) => {
                // If the path is not valid, we return an error.
                return Err(opendal::Error::new(
                    opendal::ErrorKind::PermissionDenied,
                    e.to_string(),
                ));
            }
        };
        self.inner.delete(helper.entry_path.as_str(), args)?;
        self.path_queue.push(helper);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence::files::opendal::opendal_test_operators::{
        get_memory_operator, OpendalTestOperators,
    };
    use crate::persistence::sql::user::UserRepository;
    use crate::persistence::sql::SqlDb;
    use crate::shared::user_quota::UserQuota;

    use super::*;

    fn test_user_service(db: &SqlDb, default_quota_mb: Option<u64>) -> UserService {
        UserService::new(db.clone(), default_quota_mb)
    }

    async fn get_user_data_usage(db: &SqlDb, user_pubkey: &PublicKey) -> anyhow::Result<u64> {
        let user = UserRepository::get(user_pubkey, &mut db.pool().into())
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        Ok(user.used_bytes)
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_ensure_valid_path() {
        for (_scheme, operator) in OpendalTestOperators::new().operators() {
            let db = SqlDb::test().await;
            let layer = UserQuotaLayer::new(test_user_service(&db, None));
            let operator = operator.layer(layer);

            operator
                .write("1234567890/test.txt", vec![0; 10])
                .await
                .expect_err("Should fail because the path doesn't start with a pubkey");
            let pubkey = pubky_common::crypto::Keypair::random().public_key();
            let pubkey_raw = pubkey.z32();
            // Create user with unlimited storage (None)
            UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();
            operator
                .write(format!("{}/test.txt", pubkey_raw).as_str(), vec![0; 10])
                .await
                .expect("Should succeed because the path starts with a pubkey");
            operator
                .write("test.txt", vec![0; 10])
                .await
                .expect_err("Should fail because the path doesn't start with a pubkey");

            // Read-only operations (stat, read) should not enforce path validation,
            // since the quota layer only needs to gate write operations.
            // These must work on any path, including root and non-pubkey directories.
            operator
                .stat("/")
                .await
                .expect("stat on root should succeed");
            // stat on a non-existent non-pubkey path should return NotFound,
            let err = operator
                .stat("some_dir/")
                .await
                .expect_err("should fail because path doesn't exist");
            assert_eq!(err.kind(), opendal::ErrorKind::NotFound);
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_quota_updated_write_delete() {
        let db = SqlDb::test().await;
        let layer = UserQuotaLayer::new(test_user_service(&db, None));
        let operator = get_memory_operator().layer(layer);

        let user_pubkey1 = pubky_common::crypto::Keypair::random().public_key();
        let user_pubkey1_raw = user_pubkey1.z32();
        // Create user with 1 MB quota
        UserRepository::create_with_quota_mb(&db, &user_pubkey1, 1).await;

        // Write a file and see if the user usage is updated
        operator
            .write(
                format!("{}/test.txt1", user_pubkey1_raw).as_str(),
                vec![0; 10],
            )
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 10 + FILE_METADATA_SIZE);

        // Write the same file again but with a different size
        operator
            .write(
                format!("{}/test.txt1", user_pubkey1_raw).as_str(),
                vec![0; 12],
            )
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 12 + FILE_METADATA_SIZE);

        // Write a second file and see if the user usage is updated
        operator
            .write(
                format!("{}/test.txt2", user_pubkey1_raw).as_str(),
                vec![0; 5],
            )
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 17 + 2 * FILE_METADATA_SIZE);

        // Delete the first file and see if the user usage is updated
        operator
            .delete(format!("{}/test.txt1", user_pubkey1_raw).as_str())
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 5 + FILE_METADATA_SIZE);

        // Delete the second file and see if the user usage is updated
        operator
            .delete(format!("{}/test.txt2", user_pubkey1_raw).as_str())
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 0);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_quota_rechead() {
        use crate::persistence::files::entry::entry_layer::EntryLayer;
        use crate::persistence::files::events::{EventRepository, EventsLayer, EventsService};
        use crate::persistence::sql::entry::EntryRepository;
        use crate::shared::webdav::{EntryPath, WebDavPath};

        let db = SqlDb::test().await;
        let events_service = EventsService::new(100);
        let user_quota_layer = UserQuotaLayer::new(test_user_service(&db, None));
        let entry_layer = EntryLayer::new(db.clone());
        let events_layer = EventsLayer::new(db.clone(), events_service);
        let operator = get_memory_operator()
            .layer(user_quota_layer)
            .layer(entry_layer)
            .layer(events_layer);

        let user_pubkey1 = pubky_common::crypto::Keypair::random().public_key();
        let user_pubkey1_raw = user_pubkey1.z32();
        // 1 MB quota — exactly 1,048,576 bytes including metadata.
        UserRepository::create_with_quota_mb(&db, &user_pubkey1, 1).await;
        let one_mb: usize = 1024 * 1024;
        let max_content = one_mb - FILE_METADATA_SIZE as usize;

        let file_name1 = format!("{}/test1.txt", user_pubkey1_raw);
        let entry_path1 =
            EntryPath::new(user_pubkey1.clone(), WebDavPath::new("/test1.txt").unwrap());

        // Write a file that exceeds quota (content + metadata > 1 MB) — should fail.
        operator
            .write(file_name1.as_str(), vec![0; max_content + 1])
            .await
            .expect_err("Should fail because the user quota is exceeded");
        operator
            .read(file_name1.as_str())
            .await
            .expect_err("Should fail because the file doesn't exist");
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, 0);

        // Verify that no entry was created in the database
        EntryRepository::get_by_path(&entry_path1, &mut db.pool().into())
            .await
            .expect_err("Entry should not exist because quota was exceeded");

        // Verify that no event was created in the database
        let events = crate::persistence::files::events::EventRepository::get_by_cursor(
            None,
            Some(9999),
            &mut db.pool().into(),
        )
        .await
        .expect("Should succeed");
        assert_eq!(
            events.len(),
            0,
            "No events should be created when quota is exceeded"
        );

        // Write file at exactly the quota limit — should succeed.
        operator
            .write(file_name1.as_str(), vec![0; max_content])
            .await
            .expect("Should succeed because the user quota is exactly the limit");
        operator
            .read(file_name1.as_str())
            .await
            .expect("Should succeed because the file exists");
        let user_usage = get_user_data_usage(&db, &user_pubkey1).await.unwrap();
        assert_eq!(user_usage, max_content as u64 + FILE_METADATA_SIZE);

        // Verify that entry WAS created when write succeeded
        let entry = EntryRepository::get_by_path(&entry_path1, &mut db.pool().into())
            .await
            .expect("Entry should exist after successful write");
        assert_eq!(entry.content_length as usize, max_content);

        // Verify that event WAS created when write succeeded
        let events = EventRepository::get_by_cursor(None, Some(9999), &mut db.pool().into())
            .await
            .expect("Should succeed");
        assert_eq!(
            events.len(),
            1,
            "Event should be created after successful write"
        );

        let file_name2 = format!("{}/test2.txt", user_pubkey1_raw);
        // Write a second file — even 1 byte should exceed the (now full) quota
        operator
            .write(file_name2.as_str(), vec![0; 1])
            .await
            .expect_err("Should fail because the user quota is exceeded");
    }

    /// Verify all `QuotaOverride` variants resolve correctly for storage enforcement.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_quota_override_variants() {
        use crate::shared::user_quota::QuotaOverride;

        let db = SqlDb::test().await;
        // System default: 1 MB
        let layer = UserQuotaLayer::new(test_user_service(&db, Some(1)));
        let operator = get_memory_operator().layer(layer);

        // ── Value(1) — explicit 1 MB limit ──
        let pk_value = pubky_common::crypto::Keypair::random().public_key();
        let raw_value = pk_value.z32();
        UserRepository::create_with_quota_mb(&db, &pk_value, 1).await;

        operator
            .write(format!("{raw_value}/small.txt").as_str(), vec![0; 31])
            .await
            .expect("Value(1): small write within 1 MB");
        let usage = get_user_data_usage(&db, &pk_value).await.unwrap();
        assert_eq!(usage, 31 + FILE_METADATA_SIZE);

        operator
            .write(format!("{raw_value}/huge.txt").as_str(), vec![0; 1_048_576])
            .await
            .expect_err("Value(1): >1 MB write should fail");

        // ── Value(0) — zero storage ──
        let pk_zero = pubky_common::crypto::Keypair::random().public_key();
        let raw_zero = pk_zero.z32();
        UserRepository::create_with_quota_mb(&db, &pk_zero, 0).await;

        operator
            .write(format!("{raw_zero}/file.txt").as_str(), vec![0; 1])
            .await
            .expect_err("Value(0): even 1 byte should fail");

        // ── Default — resolves to system default (1 MB) ──
        let pk_default = pubky_common::crypto::Keypair::random().public_key();
        let raw_default = pk_default.z32();
        UserRepository::create(&pk_default, &mut db.pool().into())
            .await
            .unwrap();

        operator
            .write(format!("{raw_default}/small.txt").as_str(), vec![0; 31])
            .await
            .expect("Default: small write within 1 MB system default");

        operator
            .write(
                format!("{raw_default}/huge.txt").as_str(),
                vec![0; 1_048_576],
            )
            .await
            .expect_err("Default: >1 MB write should fail against system default");

        // ── Default with no system default — unlimited ──
        let db2 = SqlDb::test().await;
        let layer_no_default = UserQuotaLayer::new(test_user_service(&db2, None));
        let op_no_default = get_memory_operator().layer(layer_no_default);

        let pk_no_default = pubky_common::crypto::Keypair::random().public_key();
        let raw_no_default = pk_no_default.z32();
        UserRepository::create(&pk_no_default, &mut db2.pool().into())
            .await
            .unwrap();

        op_no_default
            .write(
                format!("{raw_no_default}/big.txt").as_str(),
                vec![0; 2 * 1024 * 1024],
            )
            .await
            .expect("Default + no system default = unlimited");

        // ── Unlimited — bypasses system default ──
        let pk_unlimited = pubky_common::crypto::Keypair::random().public_key();
        let raw_unlimited = pk_unlimited.z32();
        let user = UserRepository::create(&pk_unlimited, &mut db.pool().into())
            .await
            .unwrap();
        let config = UserQuota {
            storage_quota_mb: QuotaOverride::Unlimited,
            ..Default::default()
        };
        UserRepository::set_quota(user.id, &config, &mut db.pool().into())
            .await
            .unwrap();

        operator
            .write(
                format!("{raw_unlimited}/big.txt").as_str(),
                vec![0; 2 * 1024 * 1024],
            )
            .await
            .expect("Unlimited: bypasses 1 MB system default");
    }

    /// Verify that changing a user's quota takes effect on the next write.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_storage_quota_change_takes_effect() {
        let db = SqlDb::test().await;
        let layer = UserQuotaLayer::new(test_user_service(&db, None));
        let operator = get_memory_operator().layer(layer);

        let user_pubkey = pubky_common::crypto::Keypair::random().public_key();
        let user_raw = user_pubkey.z32();

        // Create user with 0 MB quota (no storage)
        let user = UserRepository::create_with_quota_mb(&db, &user_pubkey, 0).await;

        // Write should fail
        operator
            .write(format!("{user_raw}/file.txt").as_str(), vec![0; 10])
            .await
            .expect_err("Should fail: zero quota");

        // Admin raises quota to 1 MB
        let config = UserQuota {
            storage_quota_mb: crate::shared::user_quota::QuotaOverride::Value(1),
            ..Default::default()
        };
        UserRepository::set_quota(user.id, &config, &mut db.pool().into())
            .await
            .unwrap();

        // Now the write should succeed
        operator
            .write(format!("{user_raw}/file.txt").as_str(), vec![0; 10])
            .await
            .expect("Should succeed after quota increase");
    }
}
