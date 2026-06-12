use std::sync::Arc;

use crate::persistence::files::layer_domain_error::LayerDomainError;
use crate::persistence::sql::{path_write_reservation::PathWriteReservationRepository, SqlDb};
use crate::shared::webdav::EntryPath;
use opendal::raw::*;
use opendal::Result;

/// OpenDAL layer that rejects app-facing file/folder path collisions before
/// opening a backend writer.
#[derive(Clone)]
pub struct PathCollisionLayer {
    db: SqlDb,
}

impl PathCollisionLayer {
    pub fn new(db: SqlDb) -> Self {
        Self { db }
    }
}

impl<A: Access> Layer<A> for PathCollisionLayer {
    type LayeredAccess = PathCollisionAccessor<A>;

    fn layer(&self, inner: A) -> Self::LayeredAccess {
        PathCollisionAccessor {
            inner: Arc::new(inner),
            db: self.db.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PathCollisionAccessor<A: Access> {
    inner: Arc<A>,
    db: SqlDb,
}

impl<A: Access> PathCollisionAccessor<A> {
    async fn reserve_path(&self, path: &str) -> Result<PathCollisionReservation> {
        let entry_path = EntryPath::parse_opendal(path)?;
        let reservation = PathWriteReservationRepository::reserve(&self.db, &entry_path)
            .await
            .map_err(|e| {
                unexpected_error(format!(
                    "Failed to reserve path collision guard for {entry_path}: {e}"
                ))
            })?
            .ok_or_else(|| path_collision_error(&entry_path))?;

        Ok(PathCollisionReservation {
            db: self.db.clone(),
            id: reservation.id,
            released: false,
        })
    }
}

struct PathCollisionReservation {
    db: SqlDb,
    id: i64,
    released: bool,
}

impl PathCollisionReservation {
    async fn release(mut self) -> Result<()> {
        let result = Self::release_by_id(&self.db, self.id).await;
        if result.is_ok() {
            self.released = true;
        }
        result
    }

    async fn release_by_id(db: &SqlDb, id: i64) -> Result<()> {
        PathWriteReservationRepository::release(db, id)
            .await
            .map_err(|e| unexpected_error(format!("Failed to release path reservation: {e}")))
    }

    fn release_in_background(db: SqlDb, id: i64) {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                drop(handle.spawn(async move {
                    if let Err(e) = Self::release_by_id(&db, id).await {
                        tracing::warn!(
                            reservation_id = id,
                            error = %e,
                            "Failed to release dropped path reservation"
                        );
                    }
                }));
            }
            Err(e) => {
                tracing::warn!(
                    reservation_id = id,
                    error = %e,
                    "Dropped path reservation outside Tokio runtime; stale cleanup will remove it later"
                );
            }
        }
    }
}

impl Drop for PathCollisionReservation {
    fn drop(&mut self) {
        if self.released {
            return;
        }

        Self::release_in_background(self.db.clone(), self.id);
        self.released = true;
    }
}

fn unexpected_error(message: impl Into<String>) -> opendal::Error {
    opendal::Error::new(opendal::ErrorKind::Unexpected, message.into())
}

fn path_collision_error(entry_path: &EntryPath) -> opendal::Error {
    opendal::Error::new(
        opendal::ErrorKind::AlreadyExists,
        format!("File/folder path collision for {entry_path}"),
    )
    .set_source(LayerDomainError::PathCollision)
}

pub struct PathCollisionWriter<R> {
    inner: R,
    reservation: Option<PathCollisionReservation>,
}

impl<R> PathCollisionWriter<R> {
    async fn release_reservation(&mut self) -> Result<()> {
        if let Some(reservation) = self.reservation.take() {
            reservation.release().await?;
        }
        Ok(())
    }
}

impl<R: oio::Write> oio::Write for PathCollisionWriter<R> {
    async fn write(&mut self, bs: opendal::Buffer) -> Result<()> {
        self.inner.write(bs).await
    }

    async fn abort(&mut self) -> Result<()> {
        let abort_result = self.inner.abort().await;
        let release_result = self.release_reservation().await;
        abort_result?;
        release_result
    }

    async fn close(&mut self) -> Result<opendal::Metadata> {
        let metadata = match self.inner.close().await {
            Ok(metadata) => metadata,
            Err(e) => {
                self.release_reservation().await?;
                return Err(e);
            }
        };
        self.release_reservation().await?;
        Ok(metadata)
    }
}

impl<A: Access> LayeredAccess for PathCollisionAccessor<A> {
    type Inner = A;
    type Reader = A::Reader;
    type Writer = PathCollisionWriter<A::Writer>;
    type Lister = A::Lister;
    type Deleter = A::Deleter;

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
        let reservation = self.reserve_path(path).await?;
        let (rp, writer) = match self.inner.write(path, args).await {
            Ok(result) => result,
            Err(e) => {
                reservation.release().await?;
                return Err(e);
            }
        };
        Ok((
            rp,
            PathCollisionWriter {
                inner: writer,
                reservation: Some(reservation),
            },
        ))
    }

    async fn copy(&self, from: &str, to: &str, args: OpCopy) -> Result<RpCopy> {
        let reservation = self.reserve_path(to).await?;
        let result = self.inner.copy(from, to, args).await;
        let release_result = reservation.release().await;
        match result {
            Ok(rp) => {
                release_result?;
                Ok(rp)
            }
            Err(e) => {
                release_result?;
                Err(e)
            }
        }
    }

    async fn rename(&self, from: &str, to: &str, args: OpRename) -> Result<RpRename> {
        let reservation = self.reserve_path(to).await?;
        let result = self.inner.rename(from, to, args).await;
        let release_result = reservation.release().await;
        match result {
            Ok(rp) => {
                release_result?;
                Ok(rp)
            }
            Err(e) => {
                release_result?;
                Err(e)
            }
        }
    }

    async fn stat(&self, path: &str, args: OpStat) -> Result<RpStat> {
        self.inner.stat(path, args).await
    }

    async fn delete(&self) -> Result<(RpDelete, Self::Deleter)> {
        self.inner.delete().await
    }

    async fn list(&self, path: &str, args: OpList) -> Result<(RpList, Self::Lister)> {
        self.inner.list(path, args).await
    }

    async fn presign(&self, path: &str, args: OpPresign) -> Result<RpPresign> {
        self.inner.presign(path, args).await
    }
}
