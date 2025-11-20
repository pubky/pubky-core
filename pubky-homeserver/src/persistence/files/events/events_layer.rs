use crate::persistence::files::events::{EventType, EventsService};
use crate::persistence::files::utils::ensure_valid_path;
use crate::persistence::files::FileMetadataBuilder;
use crate::persistence::sql::{user::UserRepository, SqlDb, UnifiedExecutor};
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

/// Layer that wraps the access layer and creates events when files are written or deleted.
/// This Layer repeats work done in entry_layer, ie calculating FileMetaData and fetching user_id. We accept this now because the idea is to remove entry_layer soon.
#[derive(Clone)]
pub struct EventsLayer {
    pub(crate) db: SqlDb,
    pub(crate) events_service: EventsService,
}

impl EventsLayer {
    pub fn new(db: SqlDb, events_service: EventsService) -> Self {
        Self { db, events_service }
    }
}

impl<A: Access> Layer<A> for EventsLayer {
    type LayeredAccess = EventsAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        EventsAccessor {
            inner,
            db: self.db.clone(),
            events_service: self.events_service.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventsAccessor<A: Access> {
    inner: A,
    db: SqlDb,
    events_service: EventsService,
}

impl<A: Access> LayeredAccess for EventsAccessor<A> {
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
                db: self.db.clone(),
                events_service: self.events_service.clone(),
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
                db: self.db.clone(),
                events_service: self.events_service.clone(),
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

/// Wrapper around the writer that creates an event when the file is closed.
pub struct WriterWrapper<R> {
    inner: R,
    db: SqlDb,
    events_service: EventsService,
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
        let metadata = self.inner.close().await?;
        let file_metadata = self.metadata_builder.clone().finalize();

        // Create event after successful write
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;
        let mut executor: UnifiedExecutor<'_> = (&mut tx).into();

        // TODO We're currently doing this is all 3 layers. Consider caching or sharing data.
        let user_id = UserRepository::get_id(self.entry_path.pubkey(), &mut executor)
            .await
            .map_err(|e| opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string()))?;

        match self
            .events_service
            .create_event(
                user_id,
                EventType::Put,
                &self.entry_path,
                Some(file_metadata.hash),
                &mut executor,
            )
            .await
        {
            Ok(event) => {
                drop(executor);
                tx.commit().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
                self.events_service.broadcast_event(event);
            }
            Err(e) => {
                drop(executor);
                tx.rollback().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
                tracing::error!(
                    "Failed to create event for {} in database: {:?}",
                    self.entry_path,
                    e
                );
                return Err(opendal::Error::new(
                    opendal::ErrorKind::Unexpected,
                    format!(
                        "Failed to create event for {} in database: {:?}",
                        self.entry_path, e
                    ),
                ));
            }
        };

        Ok(metadata)
    }
}

/// This wrapper is used to create events for deleted paths.
pub struct DeleterWrapper<R> {
    inner: R,
    db: SqlDb,
    events_service: EventsService,
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
                self.db.pool().begin().await.map_err(|e| {
                    opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                })?;
            let mut executor: UnifiedExecutor<'_> = (&mut tx).into();

            let user_id = match UserRepository::get_id(path.pubkey(), &mut executor).await {
                Ok(id) => id,
                Err(e) => {
                    tracing::error!("Failed to get user_id for {} from database: {:?}", path, e);
                    continue;
                }
            };

            match self
                .events_service
                .create_event(user_id, EventType::Delete, &path, None, &mut executor)
                .await
            {
                Ok(event) => {
                    drop(executor);
                    tx.commit().await.map_err(|e| {
                        opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                    })?;
                    self.events_service.broadcast_event(event);
                }
                Err(e) => {
                    drop(executor);
                    tx.rollback().await.map_err(|e| {
                        opendal::Error::new(opendal::ErrorKind::Unexpected, e.to_string())
                    })?;
                    tracing::error!(
                        "Failed to create delete event for {} in database: {:?}",
                        path,
                        e
                    );
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
            files::{
                events::{EventRepository, EventType, EventsService},
                opendal::opendal_test_operators::OpendalTestOperators,
            },
            sql::user::UserRepository,
        },
        shared::webdav::WebDavPath,
    };

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_events_layer() {
        for (_scheme, operator) in OpendalTestOperators::new().operators() {
            let db = SqlDb::test().await;
            let events_service = EventsService::new(100);
            let layer = EventsLayer::new(db.clone(), events_service);
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

            // Make sure the event is written to the database correctly
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

            // Make sure the event is written to the database correctly
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

            // Make sure the event is written to the database correctly
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
