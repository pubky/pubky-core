use std::collections::HashMap;
use std::sync::Arc;

use crate::persistence::lmdb::LmDB;
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

/// A rough estimate of the size of the file metadata.
/// This is added to every file.
/// This prevents the user from writing zero byte files that don't count against the quota.
const FILE_METADATA_SIZE: u64 = 256;


/// The user quota layer is a layer that wraps the operator and updates the user quota when a file is written or deleted.
/// It is used to limit the amount of data that a user can store in the homeserver.
/// It will also enforce that only paths in the form of {pubkey}/{path} are allowed.
#[derive(Clone)]
pub struct UserQuotaLayer {
    pub(crate) db: LmDB,
    /// The maximum amount of bytes that a user can store in the homeserver.
    pub(crate) user_quota_bytes: u64,
}

impl UserQuotaLayer {
    pub fn new(db: LmDB, user_quota_bytes: u64) -> Self {
        Self { db, user_quota_bytes }
    }
}

impl<A: Access> Layer<A> for UserQuotaLayer {
    type LayeredAccess = UserQuotaAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        UserQuotaAccessor {
            inner: Arc::new(inner),
            db: self.db.clone(),
            user_quota_bytes: self.user_quota_bytes,
        }
    }
}

/// Helper function to ensure that the path is a valid entry path aka 
/// starts with a pubkey.
/// Returns the entry path if it is valid, otherwise returns an error.
fn ensure_valid_path(path: &str) -> Result<EntryPath, opendal::Error> {
    let path: EntryPath = match path.parse() {
        Ok(path) => path,
        Err(e) => {
            return Err(opendal::Error::new(
                opendal::ErrorKind::PermissionDenied,
                e.to_string(),
            ));
        }
    };
    Ok(path)
}

#[derive(Debug, Clone)]
pub struct UserQuotaAccessor<A: Access> {
    inner: Arc<A>,
    db: LmDB,
    user_quota_bytes: u64,
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
        ensure_valid_path(path)?;
        self.inner.create_dir(path, args).await
    }

    async fn read(&self, path: &str, args: OpRead) -> Result<(RpRead, Self::Reader)> {
        self.inner.read(path, args).await
    }

    async fn write(&self, path: &str, args: OpWrite) -> Result<(RpWrite, Self::Writer)> {
        let entry_path = ensure_valid_path(path)?;
        let (rp, writer) = self.inner.write(path, args).await?;
        Ok((
            rp,
            WriterWrapper {
                inner: writer,
                db: self.db.clone(),
                bytes_count: 0,
                entry_path,
                inner_accessor: self.inner.clone(),
                user_quota_bytes: self.user_quota_bytes,
            },
        ))
    }

    async fn copy(&self, from: &str, to: &str, args: OpCopy) -> Result<RpCopy> {
        let _ = ensure_valid_path(to)?;
        self.inner.copy(from, to, args).await
    }

    async fn rename(&self, from: &str, to: &str, args: OpRename) -> Result<RpRename> {
        let _ = ensure_valid_path(to)?;
        self.inner.rename(from, to, args).await
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
                db: self.db.clone(),
                inner_accessor: self.inner.clone(),
                path_queue: Vec::new(),
            },
        ))
    }

    async fn list(&self, path: &str, args: OpList) -> Result<(RpList, Self::Lister)> {
        self.inner.list(path, args).await
    }

    async fn presign(&self, path: &str, args: OpPresign) -> Result<RpPresign> {
        self.inner.presign(path, args).await
    }

    type BlockingReader = A::BlockingReader;
    type BlockingWriter = A::BlockingWriter;
    type BlockingLister = A::BlockingLister;
    type BlockingDeleter = A::BlockingDeleter;

    fn blocking_read(
        &self,
        path: &str,
        args: opendal::raw::OpRead,
    ) -> opendal::Result<(opendal::raw::RpRead, Self::BlockingReader)> {
        self.inner.blocking_read(path, args)
    }

    fn blocking_write(
        &self,
        path: &str,
        args: opendal::raw::OpWrite,
    ) -> opendal::Result<(opendal::raw::RpWrite, Self::BlockingWriter)> {
        let _ = ensure_valid_path(path)?;
        self.inner.blocking_write(path, args)
    }

    fn blocking_delete(&self) -> opendal::Result<(opendal::raw::RpDelete, Self::BlockingDeleter)> {
        self.inner.blocking_delete()
    }

    fn blocking_list(
        &self,
        path: &str,
        args: opendal::raw::OpList,
    ) -> opendal::Result<(opendal::raw::RpList, Self::BlockingLister)> {
        self.inner.blocking_list(path, args)
    }
}

/// Update the user quota by the given amount.
/// This is used to update the user quota when a file is written or deleted.
/// The bytes delta is the number of bytes that were added or removed from the user quota.
/// It can be positive or negative.
fn update_user_quota(db: &LmDB, user_pubkey: &pkarr::PublicKey, bytes_delta: i64) -> anyhow::Result<()> {
    let mut wtxn = db.env.write_txn()?;
    let mut user = db
        .tables
        .users
        .get(&wtxn, user_pubkey)?
        .ok_or(anyhow::anyhow!("User not found"))?;
    user.used_bytes = user.used_bytes.saturating_add_signed(bytes_delta);
    db.tables.users.put(&mut wtxn, user_pubkey, &user)?;
    wtxn.commit()?;
    Ok(())
}

/// Wrapper around the writer that updates the user quota when the file is closed.
pub struct WriterWrapper<R, A: Access> {
    inner: R,
    db: LmDB,
    bytes_count: u64,
    entry_path: EntryPath,
    inner_accessor: Arc<A>,
    user_quota_bytes: u64,
}

impl<R, A: Access> WriterWrapper<R, A> {
    async fn get_current_file_size(&self) -> Result<(u64, bool), opendal::Error> {
        let stats = match self
            .inner_accessor
            .stat(&self.entry_path.to_string().as_str(), OpStat::default())
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
        // Update the user quota.
        let current_user_bytes = self
            .db
            .get_user_data_usage(&self.entry_path.pubkey())
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        let current_user_bytes = current_user_bytes.ok_or(opendal::Error::new(
            opendal::ErrorKind::Unexpected,
            "User not found",
        ))?;

        let (current_file_size, file_already_exists) = self.get_current_file_size().await?;

        let bytes_delta = if file_already_exists {
            self.bytes_count as i64 - current_file_size as i64
        } else {
            self.bytes_count as i64 - current_file_size as i64 + FILE_METADATA_SIZE as i64
        };

        // Check if the user quota is exceeded before we commit/close the file.
        if current_user_bytes as i64 + bytes_delta > self.user_quota_bytes as i64 {
            return Err(opendal::Error::new(
                opendal::ErrorKind::RateLimited,
                "User quota exceeded",
            ));
        }
        let metadata = self.inner.close().await?;
        update_user_quota(&self.db, &self.entry_path.pubkey(), bytes_delta)
            .map_err(|e| {
                opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
            })?;
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
        let size = match operator.stat(&self.entry_path.as_str(), OpStat::default()).await {
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
    db: LmDB,
    inner_accessor: Arc<A>,
    path_queue: Vec<DeletePath>,
}

impl<R, A: Access> DeleterWrapper<R, A> {
    fn update_user_quota(&self, deleted_paths: Vec<DeletePath>) -> Result<()> {
        // Group deleted paths by user pubkey
        let mut user_paths: HashMap<pkarr::PublicKey, Vec<DeletePath>> = HashMap::new();
        for path in deleted_paths {
            user_paths
                .entry(path.entry_path.pubkey().clone())
                .or_insert_with(Vec::new)
                .push(path);
        }

        // TODO: Update user quota for each user
        for (user_pubkey, paths) in user_paths {
            let total_bytes: u64 = paths.iter().filter_map(|p| p.bytes_count).sum();
            let files_deleted_count = paths.iter().filter(|p| p.exists.unwrap_or(false)).count() as u64;
            let bytes_delta = (total_bytes + files_deleted_count * FILE_METADATA_SIZE) as i64;
            update_user_quota(&self.db, &user_pubkey, -bytes_delta)
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
        self.update_user_quota(deleted_paths)?;
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
        self.inner.delete(path, args)?;
        self.path_queue.push(helper);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::opendal_test_operators::{get_memory_operator, OpendalTestOperators};

    use super::*;

    fn get_user_data_usage(db: &LmDB, user_pubkey: &pkarr::PublicKey) -> anyhow::Result<u64> {
        let wtxn = db.env.read_txn()?;
        let user = db.get_user(user_pubkey, &wtxn)?.ok_or(opendal::Error::new(
            opendal::ErrorKind::Unexpected,
            "User not found",
        ))?;
        Ok(user.used_bytes)
    }

    #[tokio::test]
    async fn test_ensure_valid_path() {
        for (_scheme, operator) in OpendalTestOperators::new().operators() {
            let db = LmDB::test();
            let layer = UserQuotaLayer::new(db.clone(), 1024 * 1024);
            let operator = operator.layer(layer);
    
            operator.write("1234567890/test.txt", vec![0; 10]).await.expect_err("Should fail because the path doesn't start with a pubkey");
            let pubkey = pkarr::Keypair::random().public_key();
            db.create_user(&pubkey).unwrap();
            operator.write(format!("{}/test.txt", pubkey).as_str(), vec![0; 10]).await.expect("Should succeed because the path starts with a pubkey");
            operator.write("test.txt", vec![0; 10]).await.expect_err("Should fail because the path doesn't start with a pubkey");
        };
    }

    #[tokio::test]
    async fn test_quota_updated_write_delete() {
        let db = LmDB::test();
        let layer = UserQuotaLayer::new(db.clone(), 1024 * 1024);
        let operator = get_memory_operator().layer(layer);

        let user_pubkey1 = pkarr::Keypair::random().public_key();
        db.create_user(&user_pubkey1).unwrap();

        // Write a file and see if the user usage is updated
        operator
            .write(format!("{}/test.txt1", user_pubkey1).as_str(), vec![0; 10])
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 10 + FILE_METADATA_SIZE);

        // Write the same file again but with a different size
        operator
            .write(format!("{}/test.txt1", user_pubkey1).as_str(), vec![0; 12])
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 12 + FILE_METADATA_SIZE);

        // Write a second file and see if the user usage is updated
        operator
            .write(
                format!("{}/test.txt2", user_pubkey1).as_str(),
                vec![0; 5],
            )
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 17 + 2* FILE_METADATA_SIZE);

        // Delete the first file and see if the user usage is updated
        operator.delete(format!("{}/test.txt1", user_pubkey1).as_str()).await.unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 5 + FILE_METADATA_SIZE);

        // Delete the second file and see if the user usage is updated
        operator.delete(format!("{}/test.txt2", user_pubkey1).as_str()).await.unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 0);
    }

    #[tokio::test]
    async fn test_quota_rechead() {
        let db = LmDB::test();
        let layer = UserQuotaLayer::new(db.clone(), 20 + FILE_METADATA_SIZE);
        let operator = get_memory_operator().layer(layer);

        let user_pubkey1 = pkarr::Keypair::random().public_key();
        db.create_user(&user_pubkey1).unwrap();

        let file_name1 = format!("{}/test1.txt", user_pubkey1);
        // Write a file and see if the user usage is updated
        operator
            .write(file_name1.as_str(), vec![0; 21])
            .await
            .expect_err("Should fail because the user quota is exceeded");
        operator.read(file_name1.as_str()).await.expect_err("Should fail because the file doesn't exist");
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 0);

        // Write file at exactly the quota limit
        operator
        .write(file_name1.as_str(), vec![0; 20])
        .await
        .expect("Should succeed because the user quota is exactly the limit");
        operator.read(file_name1.as_str()).await.expect("Should succeed because the file exists");
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 20 + FILE_METADATA_SIZE);


        let file_name2 = format!("{}/test2.txt", user_pubkey1);
        // Write a second file and see if the user usage is updated
        operator
            .write(file_name2.as_str(), vec![0; 1])
            .await
            .expect_err("Should fail because the user quota is exceeded");
    }
}
