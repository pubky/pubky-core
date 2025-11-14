use crate::persistence::files::entry_service::EntryService;
use crate::persistence::files::events_service::EventsService;
use crate::persistence::files::FileMetadataBuilder;
use crate::persistence::sql::{SqlDb, UnifiedExecutor};
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
    pub(crate) db: SqlDb,
    pub(crate) events_service: EventsService,
}

impl EntryLayer {
    pub fn new(db: SqlDb, events_service: EventsService) -> Self {
        Self { db, events_service }
    }
}

impl<A: Access> Layer<A> for EntryLayer {
    type LayeredAccess = EntryAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        EntryAccessor {
            inner,
            entry_service: EntryService::new(self.db.clone(), self.events_service.clone()),
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
        let metadata = self.inner.close().await?; // Write the file to the storage.
                                                  // Write successful, update the entry in the database.
        let file_metadata = self.metadata_builder.clone().finalize();
        let mut tx = self
            .entry_service
            .db()
            .pool()
            .begin()
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        let mut executor: UnifiedExecutor<'_> = (&mut tx).into();
        match self
            .entry_service
            .write_entry(&self.entry_path, &file_metadata, &mut executor)
            .await
        {
            Ok((_entry, event)) => {
                drop(executor);
                tx.commit().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
                // Broadcast event after successful commit
                self.entry_service.broadcast_event(event);
            }
            Err(e) => {
                drop(executor);
                tx.rollback().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
                tracing::error!(
                    "Failed to write entry {} to database: {:?}. Potential orphaned file.",
                    self.entry_path,
                    e
                );
                return Err(opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    format!(
                        "Failed to write entry {} to database: {:?}. Potential orphaned file.",
                        self.entry_path, e
                    ),
                ));
            }
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
            let mut tx =
                self.entry_service.db().pool().begin().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
            let mut executor: UnifiedExecutor<'_> = (&mut tx).into();

            match self.entry_service.delete_entry(&path, &mut executor).await {
                Ok(event) => {
                    drop(executor);
                    tx.commit().await.map_err(|e| {
                        opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                    })?;
                    // Broadcast event after successful commit
                    self.entry_service.broadcast_event(event);
                }
                Err(e) => {
                    drop(executor);
                    tx.rollback().await.map_err(|e| {
                        opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                    })?;
                    tracing::error!("Failed to delete entry {} from database: {:?}", path, e);
                }
            };
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
        persistence::{
            files::opendal_test_operators::OpendalTestOperators,
            sql::{
                entry::EntryRepository,
                event::{EventRepository, EventType},
                user::UserRepository,
            },
        },
        shared::webdav::WebDavPath,
    };

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_entry_layer() {
        for (_scheme, operator) in OpendalTestOperators::new().operators() {
            let db = SqlDb::test().await;
            let events_service = EventsService::new(100);
            let layer = EntryLayer::new(db.clone(), events_service);
            let operator = operator.layer(layer);

            let pubkey = pkarr::Keypair::random().public_key();
            UserRepository::create(&pubkey, &mut db.pool().into())
                .await
                .unwrap();
            let path = WebDavPath::new("/test.txt").unwrap();
            let entry_path = EntryPath::new(pubkey, path);
            operator
                .write(entry_path.as_str(), vec![0; 10])
                .await
                .expect("Should succeed because the path starts with a pubkey");

            // Make sure the entry is written to the database correctly
            let entry = EntryRepository::get_by_path(&entry_path, &mut db.pool().into())
                .await
                .expect("Entry should exist");
            assert_eq!(entry.content_length, 10);
            let events = EventRepository::get_by_cursor(None, Some(9999), &mut db.pool().into())
                .await
                .expect("Should succeed");
            assert_eq!(events.len(), 1);
            let first_event = events.first().expect("Should succeed");
            assert_eq!(first_event.path, entry_path);
            assert_eq!(first_event.event_type, EventType::Put);

            // Overwrite the file
            operator
                .write(entry_path.as_str(), vec![0; 20])
                .await
                .expect("Should succeed because the path starts with a pubkey");

            // Make sure the entry is written to the database correctly
            let entry = EntryRepository::get_by_path(&entry_path, &mut db.pool().into())
                .await
                .expect("Entry should exist");
            assert_eq!(entry.content_length, 20);
            let events = EventRepository::get_by_cursor(None, Some(9999), &mut db.pool().into())
                .await
                .expect("Should succeed");
            assert_eq!(events.len(), 2);
            let second_event = events.get(1).expect("Should succeed");
            assert_eq!(second_event.path, entry_path);
            assert_eq!(second_event.event_type, EventType::Put);

            // Delete the file
            operator
                .delete(entry_path.as_str())
                .await
                .expect("Should succeed");

            // Make sure the entry is deleted from the database correctly
            let _entry = EntryRepository::get_by_path(&entry_path, &mut db.pool().into())
                .await
                .expect_err("Entry should not exist");
            let events = EventRepository::get_by_cursor(None, Some(9999), &mut db.pool().into())
                .await
                .expect("Should succeed");
            assert_eq!(events.len(), 3);
            let third_event = events.get(2).expect("Should succeed");
            assert_eq!(third_event.path, entry_path);
            assert_eq!(third_event.event_type, EventType::Delete);
        }
    }
}
