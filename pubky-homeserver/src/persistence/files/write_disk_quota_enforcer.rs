use bytes::Bytes;
use futures_util::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

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
    ) -> anyhow::Result<Self> {
        let existing_entry_bytes = db.get_entry_content_length(path)?;
        let user_already_used_bytes = db.get_user_data_usage(path.pubkey())?;
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
    fn err_if_exceeded_quota(&self) -> anyhow::Result<()> {
        if !self.has_exceeded_quota() {
            return Ok(());
        }
        let bytes_in_mb = 1024.0 * 1024.0;
        let max_allowed_mb = self.max_allowed_bytes as f64 / bytes_in_mb;
        anyhow::bail!(
            "Disk space quota of {max_allowed_mb:.1} MB exceeded. Write operation failed."
        );
    }
}

impl<S> Stream for WriteDiskQuotaEnforcer<S>
where
    S: Stream<Item = Result<Bytes, anyhow::Error>> + Unpin + Send,
{
    type Item = Result<Bytes, anyhow::Error>;

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
