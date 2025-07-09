use crate::persistence::files::entry_service::EntryService;
use crate::persistence::files::FileMetadataBuilder;
use crate::persistence::lmdb::LmDB;
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

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

/// Layer that wraps the access layer and updates the entry in the database when the file is written or deleted.
#[derive(Clone)]
pub struct EntryLayer {
    pub(crate) db: LmDB,
}

impl EntryLayer {
    pub fn new(db: LmDB) -> Self {
        Self { db }
    }
}

impl<A: Access> Layer<A> for EntryLayer {
    type LayeredAccess = EntryAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        EntryAccessor {
            inner,
            entry_service: EntryService::new(self.db.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EntryAccessor<A: Access> {
    inner: A,
    entry_service: EntryService,
}

impl<A: Access> LayeredAccess for EntryAccessor<A> {
    type Inner = A;
    type Reader = A::Reader;
    type Writer = WriterWrapper<A::Writer>;
    type Lister = A::Lister;
    type Deleter = DeleterWrapper<A::Deleter>;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    async fn create_dir(&self, path: &str, args: OpCreateDir) -> Result<RpCreateDir> {
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
                entry_service: self.entry_service.clone(),
                entry_path,
                metadata_builder: FileMetadataBuilder::default(),
            },
        ))
    }

    async fn copy(&self, from: &str, to: &str, args: OpCopy) -> Result<RpCopy> {
        self.inner.copy(from, to, args).await
    }

    async fn rename(&self, from: &str, to: &str, args: OpRename) -> Result<RpRename> {
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
                entry_service: self.entry_service.clone(),
                delete_queue: Vec::new(),
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
        _path: &str,
        _args: opendal::raw::OpWrite,
    ) -> opendal::Result<(opendal::raw::RpWrite, Self::BlockingWriter)> {
        Err(opendal::Error::new(
            opendal::ErrorKind::Unsupported,
            "Writing is not supported in blocking mode",
        ))
    }

    fn blocking_delete(&self) -> opendal::Result<(opendal::raw::RpDelete, Self::BlockingDeleter)> {
        Err(opendal::Error::new(
            opendal::ErrorKind::Unsupported,
            "Deleting is not supported in blocking mode",
        ))
    }

    fn blocking_list(
        &self,
        path: &str,
        args: opendal::raw::OpList,
    ) -> opendal::Result<(opendal::raw::RpList, Self::BlockingLister)> {
        self.inner.blocking_list(path, args)
    }
}

/// Wrapper around the writer that updates the entry in the database when the file is closed.
pub struct WriterWrapper<R> {
    inner: R,
    entry_service: EntryService,
    entry_path: EntryPath,
    metadata_builder: FileMetadataBuilder,
}

impl<R: oio::Write> oio::Write for WriterWrapper<R> {
    async fn write(&mut self, bs: opendal::Buffer) -> Result<()> {
        let slice = bs.to_vec();
        self.metadata_builder.update(&slice);
        self.inner.write(bs).await
    }

    async fn abort(&mut self) -> Result<()> {
        self.inner.abort().await
    }

    async fn close(&mut self) -> Result<opendal::Metadata> {
        self.metadata_builder
            .guess_mime_type_from_path(self.entry_path.path().as_str());
        let metadata = self.inner.close().await?;
        // Write successful, update the entry in the database.
        let file_metadata = self.metadata_builder.clone().finalize();
        if let Err(e) = self
            .entry_service
            .write_entry(&self.entry_path, &file_metadata)
        {
            tracing::error!(
                "Failed to write entry {} to database: {:?}. Potential orphaned file.",
                self.entry_path,
                e
            );
        };
        Ok(metadata)
    }
}

/// This wrapper is used to delete paths in a queue
/// and update the user quota.
/// Depending on the service backend, each file is deleted in a separate request (filesystem, inmemory)
/// or batched (GCS does 100 paths per batch).
pub struct DeleterWrapper<R> {
    inner: R,
    entry_service: EntryService,
    delete_queue: Vec<EntryPath>,
}

impl<R: oio::Delete> oio::Delete for DeleterWrapper<R> {
    async fn flush(&mut self) -> Result<usize> {
        let deleted_files_count = self.inner.flush().await?;

        let deleted_paths = self
            .delete_queue
            .drain(0..deleted_files_count)
            .collect::<Vec<_>>();

        for path in deleted_paths {
            if let Err(e) = self.entry_service.delete_entry(&path) {
                tracing::error!("Failed to delete entry {} from database: {:?}", path, e);
            }
        }
        Ok(deleted_files_count)
    }

    fn delete(&mut self, path: &str, args: OpDelete) -> Result<()> {
        let entry_path = ensure_valid_path(path)?;
        self.inner.delete(path, args)?;
        self.delete_queue.push(entry_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        persistence::files::opendal_test_operators::OpendalTestOperators,
        shared::webdav::WebDavPath,
    };

    use super::*;

    #[tokio::test]
    async fn test_entry_layer() {
        for (_scheme, operator) in OpendalTestOperators::new().operators() {
            let db = LmDB::test();
            let layer = EntryLayer::new(db.clone());
            let operator = operator.layer(layer);

            let pubkey = pkarr::Keypair::random().public_key();
            let path = WebDavPath::new("/test.txt").unwrap();
            let entry_path = EntryPath::new(pubkey, path);
            operator
                .write(entry_path.as_str(), vec![0; 10])
                .await
                .expect("Should succeed because the path starts with a pubkey");

            // Make sure the entry is written to the database correctly
            let entry = db.get_entry(&entry_path).expect("Entry should exist");
            assert_eq!(entry.content_length(), 10);
            let events = db.list_events(None, None).expect("Should succeed");
            assert_eq!(events.len(), 2);
            assert_eq!(events[0], format!("PUT pubky://{}", entry_path.as_str()));

            // Overwrite the file
            operator
                .write(entry_path.as_str(), vec![0; 20])
                .await
                .expect("Should succeed because the path starts with a pubkey");

            // Make sure the entry is written to the database correctly
            let entry = db.get_entry(&entry_path).expect("Entry should exist");
            assert_eq!(entry.content_length(), 20);
            let events = db.list_events(None, None).expect("Should succeed");
            assert_eq!(events.len(), 3);
            assert_eq!(events[1], format!("PUT pubky://{}", entry_path.as_str()));

            // Delete the file
            operator
                .delete(entry_path.as_str())
                .await
                .expect("Should succeed");

            // Make sure the entry is deleted from the database correctly
            db.get_entry(&entry_path)
                .expect_err("Entry should not exist");
            let events = db.list_events(None, None).expect("Should succeed");
            assert_eq!(events.len(), 4);
            assert_eq!(events[2], format!("DEL pubky://{}", entry_path.as_str()));
        }
    }
}
