use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::{FileIoError, WriteStreamError};
use crate::persistence::lmdb::tables::users::UserQueryError;
use crate::persistence::lmdb::LmDB;
use crate::shared::webdav::EntryPath;

/// Checks if the content-size hint already exceeds the quota.
/// This is not reliable because the user might supply a fake size hint
/// but it can be used for error messages and to fail the upload early.
pub fn is_size_hint_exceeding_quota(
    content_size_hint: u64,
    db: &LmDB,
    path: &EntryPath,
    max_allowed_bytes: u64,
) -> anyhow::Result<bool> {
    let existing_entry_bytes = db.get_entry_content_length(path)?;
    let user_already_used_bytes = db.get_user_data_usage(path.pubkey())?;
    return Ok(
        user_already_used_bytes + content_size_hint.saturating_sub(existing_entry_bytes)
            > max_allowed_bytes,
    );
}

/// A stream wrapper that enforces the user max disk space limit.
/// For example, the user has only 1GB of disk space allowance but uploads a 2GB file..
pub struct WriteDiskQuotaEnforcer<S> {
    /// The stream to wrap
    inner: S,
    /// The number of bytes that this entry already takes on disk. Aka file already exists.
    existing_entry_bytes: u64,
    /// The number of bytes already seen in the stream.
    stream_byte_counter: u64,
    /// The number of bytes already used by the user.
    user_already_used_bytes: u64,
    /// The maximum number of bytes a user is allowed to write. None means no limit.
    max_allowed_bytes: u64,
}

impl<S> WriteDiskQuotaEnforcer<S> {
    pub fn new(
        inner: S,
        db: &LmDB,
        path: &EntryPath,
        max_allowed_bytes: u64,
    ) -> Result<Self, FileIoError> {
        let existing_entry_bytes = db.get_entry_content_length(path)?;
        let user_already_used_bytes = match db.get_user_data_usage(path.pubkey()) {
            Ok(count) => count,
            Err(UserQueryError::UserNotFound) => {
                // If the user doesn't exist then the file is not there either.
                // Data consistency error. Should not be able to write a file to a non-existing user.
                tracing::error!("User not found for path: {}", path);
                return Err(FileIoError::NotFound);
            }
            Err(UserQueryError::DatabaseError(e)) => {
                return Err(FileIoError::Db(e));
            }
        };

        Ok(Self {
            inner,
            existing_entry_bytes,
            stream_byte_counter: 0,
            user_already_used_bytes,
            max_allowed_bytes,
        })
    }

    /// Returns true if the user exceeds the quota.
    fn has_exceeded_quota(&self) -> bool {
        return self.user_already_used_bytes
            + self
                .stream_byte_counter
                .saturating_sub(self.existing_entry_bytes)
            > self.max_allowed_bytes;
    }

    /// Returns an error if the user exceeded the quota.
    fn err_if_exceeded_quota(&self) -> Result<(), WriteStreamError> {
        if !self.has_exceeded_quota() {
            return Ok(());
        }
        Err(WriteStreamError::DiskSpaceQuotaExceeded)
    }
}

impl<S> Stream for WriteDiskQuotaEnforcer<S>
where
    S: Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send,
{
    type Item = Result<Bytes, WriteStreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.stream_byte_counter += chunk.len() as u64;
                if let Err(e) = self.err_if_exceeded_quota() {
                    Poll::Ready(Some(Err(e.into())))
                } else {
                    Poll::Ready(Some(Ok(chunk)))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::webdav::{EntryPath, WebDavPath};
    use bytes::Bytes;
    use futures_util::{stream, StreamExt};

    // // Create a mock stream with specified chunks
    // fn create_test_stream(
    //     chunks: Vec<usize>,
    // ) -> FileStream {
    //     let byte_chunks: Vec<Result<Bytes, std::io::Error>> = chunks
    //         .into_iter()
    //         .map(|size| Ok(Bytes::from(vec![0u8; size])))
    //         .collect();
    //     Box::new(stream::iter(byte_chunks))
    // }

    // Helper function to create a test enforcer with real LmDB
    fn create_test_enforcer_with_db(
        chunks: Vec<usize>,
        db: &LmDB,
        path: &EntryPath,
        max_allowed_bytes: u64,
    ) -> Result<WriteDiskQuotaEnforcer<impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send>, FileIoError> {
        let byte_chunks: Vec<Result<Bytes, WriteStreamError>> = chunks
        .into_iter()
        .map(|size| Ok(Bytes::from(vec![0u8; size])))
        .collect();
        let test_stream = stream::iter(byte_chunks);
        let enforcer = WriteDiskQuotaEnforcer::new(test_stream, db, path, max_allowed_bytes)?;
        Ok(enforcer)
    }

    async fn consume_enforcer(
            mut enforcer: WriteDiskQuotaEnforcer<impl Stream<Item = Result<Bytes, WriteStreamError>> + Unpin + Send>,
    ) -> anyhow::Result<(bool, usize)> {
        let mut total_bytes = 0;
        let mut got_error = false;
        while let Some(result) = enforcer.next().await {
            match result {
                Ok(chunk) => total_bytes += chunk.len(),
                Err(e) => {
                    match e {
                        WriteStreamError::DiskSpaceQuotaExceeded => got_error = true,
                        _ => return Err(e.into()),
                    }
                    break;
                }
            }
        }
        Ok((got_error, total_bytes))
    }

    #[tokio::test]
    async fn allows_exact_quota() -> anyhow::Result<()> {
        // existing=0, incoming=1024, used=0, quota=1024
        // This should succeed as we're exactly at the quota limit
        let db = LmDB::test();
        let pubkey = pkarr::Keypair::random().public_key();
        let path = EntryPath::new(pubkey, WebDavPath::new("/test/file.txt")?);
        // Create a user
        let mut wtxn = db.env.write_txn()?;
        db.create_user(&path.pubkey(), &mut wtxn)?;
        wtxn.commit()?;

        let enforcer = create_test_enforcer_with_db(vec![1024], &db, &path, 1024)?;
        let (got_error, total_bytes) = consume_enforcer(enforcer).await?;
        assert!(!got_error, "Should not error when at exact quota");
        assert_eq!(total_bytes, 1024);
        Ok(())
    }

    #[tokio::test]
    async fn blocks_when_over_quota() -> anyhow::Result<()> {
        // existing=0, incoming=1025, used=0, quota=1024
        // This should fail when we exceed the quota
        let db = LmDB::test();
        let pubkey = pkarr::Keypair::random().public_key();
        let path = EntryPath::new(pubkey, WebDavPath::new("/test/file.txt")?);
        // Create a user
        let mut wtxn = db.env.write_txn()?;
        db.create_user(&path.pubkey(), &mut wtxn)?;
        wtxn.commit()?;

        let enforcer = create_test_enforcer_with_db(vec![1025], &db, &path, 1024)?;
        let (got_error, _total_bytes) = consume_enforcer(enforcer).await?;
        assert!(got_error, "Should error when over quota");
        Ok(())
    }

    #[tokio::test]
    async fn considers_existing_size() -> anyhow::Result<()> {
        // existing=800, incoming=600, used=0, quota=1000
        // Net change = max(0, 600-800) = 0, so this should succeed
        let mut db = LmDB::test();
        let pubkey = pkarr::Keypair::random().public_key();
        let path = EntryPath::new(pubkey, WebDavPath::new("/test/file.txt")?);

        // Create a user and set up existing entry by actually writing a file
        let mut wtxn = db.env.write_txn()?;
        db.create_user(&path.pubkey(), &mut wtxn)?;
        wtxn.commit()?;

        // Write an existing file of 800 bytes
        let existing_file =
            crate::persistence::lmdb::tables::files::InDbTempFile::zeros(800).await?;
        db.write_entry_from_file_sync(&path, &existing_file)?;

        let enforcer = create_test_enforcer_with_db(vec![1000], &db, &path, 1000)?;
        let (got_error, total_bytes) = consume_enforcer(enforcer).await?;
        assert!(
            !got_error,
            "Should not error when replacing bigger file below the quota"
        );
        assert_eq!(total_bytes, 1000);
        Ok(())
    }

    #[tokio::test]
    async fn blocks_when_existing_plus_new_exceeds_quota() -> anyhow::Result<()> {
        // existing=500, incoming=600, used=950, quota=1000
        // Net usage = 950 + max(0, 600-500) = 950 + 100 = 1050, which is over quota
        let mut db = LmDB::test();
        let pubkey = pkarr::Keypair::random().public_key();
        let path = EntryPath::new(pubkey.clone(), WebDavPath::new("/test/file.txt")?);

        // Create user and set up data
        let mut wtxn = db.env.write_txn()?;
        db.create_user(&path.pubkey(), &mut wtxn)?;
        wtxn.commit()?;

        // Set user usage to 450 bytes
        db.update_data_usage(&path.pubkey(), 450)?;

        // Write an existing file of 500 bytes
        let existing_file =
            crate::persistence::lmdb::tables::files::InDbTempFile::zeros(500).await?;
        db.write_entry_from_file_sync(&path, &existing_file)?;

        // Set user usage to 100 bytes
        db.update_data_usage(&path.pubkey(), 500)?;

        let enforcer = create_test_enforcer_with_db(vec![600], &db, &path, 1000)?;
        let (got_error, _total_bytes) = consume_enforcer(enforcer).await?;
        assert!(got_error, "Should error when over quota");
        Ok(())
    }
}
