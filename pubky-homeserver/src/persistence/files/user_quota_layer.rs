use std::collections::HashMap;
use std::sync::Arc;

use crate::persistence::lmdb::LmDB;
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

#[derive(Clone)]
pub struct UserQuotaLayer {
    pub(crate) db: LmDB,
    pub(crate) max_user_bytes: u64,
}

impl UserQuotaLayer {
    pub fn new(db: LmDB, max_user_bytes: u64) -> Self {
        Self { db, max_user_bytes }
    }
}

impl<A: Access> Layer<A> for UserQuotaLayer {
    type LayeredAccess = UserQuotaAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        UserQuotaAccessor {
            inner: Arc::new(inner),
            db: self.db.clone(),
            max_user_bytes: self.max_user_bytes,
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
    max_user_bytes: u64,
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
                max_user_bytes: self.max_user_bytes,
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

// #[derive(Debug, thiserror::Error)]
// enum UserQuotaError {
//     #[error("User not found")]
//     UserNotFound,
//     #[error("Database error: {0}")]
//     Database(heed::Error),
//     #[error("User quota exceeded.")]
//     QuotaExceeded,
// }

pub struct WriterWrapper<R, A: Access> {
    inner: R,
    db: LmDB,
    bytes_count: u64,
    entry_path: EntryPath,
    inner_accessor: Arc<A>,
    max_user_bytes: u64,
}

impl<R, A: Access> WriterWrapper<R, A> {
    fn update_user_quota(&self, bytes_delta: u64) -> anyhow::Result<()> {
        let mut wtxn = self.db.env.write_txn()?;
        let mut user = self
            .db
            .tables
            .users
            .get(&wtxn, &self.entry_path.pubkey())?
            .ok_or(anyhow::anyhow!("User not found"))?;
        user.used_bytes = user.used_bytes.saturating_sub(bytes_delta);
        self.db
            .tables
            .users
            .put(&mut wtxn, &self.entry_path.pubkey(), &user)?;
        wtxn.commit()?;
        Ok(())
    }

    async fn get_current_file_size(&self) -> Result<u64, opendal::Error> {
        let stats = match self
            .inner_accessor
            .stat(&self.entry_path.to_string().as_str(), OpStat::default())
            .await
        {
            Ok(stats) => stats,
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                // If the file does not exist, we assume it was deleted
                // and we don't count it against the user quota
                return Ok(0);
            }
            Err(e) => {
                return Err(e);
            }
        };
        let file_size = stats.into_metadata().content_length();
        Ok(file_size)
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
        let current_user_usage = self
            .db
            .get_user_data_usage(&self.entry_path.pubkey())
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        let current_user_usage = current_user_usage.ok_or(opendal::Error::new(
            opendal::ErrorKind::Unexpected,
            "User not found",
        ))?;

        let current_file_size = self.get_current_file_size().await?;

        if self.bytes_count + current_user_usage - current_file_size > self.max_user_bytes {
            return Err(opendal::Error::new(
                opendal::ErrorKind::RateLimited,
                "User quota exceeded",
            ));
        }
        self.update_user_quota(self.bytes_count - current_file_size)
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        println!("Closing writer with bytes count: {}", self.bytes_count);
        match self.inner.close().await {
            Ok(metadata) => {
                self.update_user_quota(self.bytes_count - current_file_size)
                    .map_err(|e| {
                        opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                    })?;
                Ok(metadata)
            }
            Err(e) => Err(e),
        }
    }
}

/// Helper struct to store the path and the bytes count of a path.
struct DeletePath {
    path: String,
    bytes_count: Option<u64>,
    user_pubkey: pkarr::PublicKey,
}

impl DeletePath {
    fn new(path: &str) -> anyhow::Result<Self> {
        let pubkey = Self::extract_pubkey(path)?;
        Ok(Self {
            path: path.to_string(),
            bytes_count: None,
            user_pubkey: pubkey,
        })
    }

    /// Extract the pubkey from the path.
    /// Must be the first part of the path.
    fn extract_pubkey(path: &str) -> anyhow::Result<pkarr::PublicKey> {
        let parts = path.split('/').collect::<Vec<&str>>();
        if parts.len() < 1 {
            return Err(anyhow::anyhow!("Path must contain a pubkey"));
        }
        let pubkey = parts[0];
        let pubkey: pkarr::PublicKey = pubkey.parse()?;
        Ok(pubkey)
    }

    /// Pull the bytes count of the path.
    pub async fn pull_bytes_count<A: Access>(&mut self, operator: &A) -> Result<()> {
        if self.bytes_count.is_some() {
            // Already got the bytes count
            return Ok(());
        }
        let size = match operator.stat(&self.path, OpStat::default()).await {
            Ok(stats) => stats.into_metadata().content_length(),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                // If the file does not exist, we assume it was deleted
                // and we don't count it against the user quota
                return Ok(());
            }
            Err(e) => {
                return Err(e);
            }
        };
        self.bytes_count = Some(size);
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
                .entry(path.user_pubkey.clone())
                .or_insert_with(Vec::new)
                .push(path);
        }

        // TODO: Update user quota for each user
        for (user_pubkey, paths) in user_paths {
            let total_bytes: u64 = paths.iter().filter_map(|p| p.bytes_count).sum();
            println!(
                "User {} deleted {} bytes across {} files",
                user_pubkey,
                total_bytes,
                paths.len()
            );

            self.decrease_user_quota(&user_pubkey, total_bytes)
                .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        }

        Ok(())
    }

    /// Decrease the user quota by the given amount.
    fn decrease_user_quota(
        &self,
        user_pubkey: &pkarr::PublicKey,
        bytes_delta: u64,
    ) -> anyhow::Result<()> {
        let mut wtxn = self.db.env.write_txn()?;
        let mut user = self
            .db
            .tables
            .users
            .get(&wtxn, user_pubkey)?
            .ok_or(anyhow::anyhow!("User not found"))?;
        user.used_bytes = user.used_bytes.saturating_sub(bytes_delta);
        self.db.tables.users.put(&mut wtxn, user_pubkey, &user)?;
        wtxn.commit()?;
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
        let helper = match DeletePath::new(path) {
            Ok(helper) => helper,
            Err(e) => {
                return Err(opendal::Error::new(
                    opendal::ErrorKind::PermissionDenied,
                    e.to_string(),
                ));
            }
        };
        self.path_queue.push(helper);
        self.inner.delete(path, args)
    }
}

#[cfg(test)]
mod tests {
    use crate::shared::opendal_test_operators::OpendalTestOperators;

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
    async fn test_deleter_wrapper() {
        let db = LmDB::test();
        let layer = UserQuotaLayer::new(db.clone(), 1024 * 1024);
        let builder = opendal::services::Memory::default();
        let operator = opendal::Operator::new(builder)
            .unwrap()
            .layer(layer)
            .finish();

        let user_pubkey1 = pkarr::Keypair::random().public_key();
        db.create_user(&user_pubkey1).unwrap();

        // Write a file and see if the user usage is updated
        operator
            .write(format!("{}/test.txt1", user_pubkey1).as_str(), vec![0; 10])
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 10);

        // Write the same file again but with a different size
        operator
            .write(format!("{}/test.txt1", user_pubkey1).as_str(), vec![0; 12])
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 12);

        // Write a second file and see if the user usage is updated
        operator
            .write(
                format!("{}/test.txt2", user_pubkey1).as_str(),
                vec![0; 5],
            )
            .await
            .unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 17);

        // Delete the first file and see if the user usage is updated
        operator.delete(format!("{}/test.txt1", user_pubkey1).as_str()).await.unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 5);

        // Delete the second file and see if the user usage is updated
        operator.delete(format!("{}/test.txt2", user_pubkey1).as_str()).await.unwrap();
        let user_usage = get_user_data_usage(&db, &user_pubkey1).unwrap();
        assert_eq!(user_usage, 0);
    }
}
