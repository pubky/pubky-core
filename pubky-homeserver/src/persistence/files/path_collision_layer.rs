use std::sync::Arc;

use crate::persistence::files::layer_domain_error::LayerDomainError;
use crate::persistence::sql::{entry::EntryRepository, SqlDb};
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
    async fn check_no_path_collision(&self, path: &str) -> Result<()> {
        let entry_path = EntryPath::parse_opendal(path)?;
        let has_collision =
            EntryRepository::has_file_folder_collision(&entry_path, &mut self.db.pool().into())
                .await
                .map_err(|e| {
                    opendal::Error::new(
                        opendal::ErrorKind::Unexpected,
                        format!("Failed to check path collision for {entry_path}: {e}"),
                    )
                })?;

        if has_collision {
            return Err(opendal::Error::new(
                opendal::ErrorKind::AlreadyExists,
                format!("File/folder path collision for {entry_path}"),
            )
            .set_source(LayerDomainError::PathCollision));
        }

        Ok(())
    }
}

impl<A: Access> LayeredAccess for PathCollisionAccessor<A> {
    type Inner = A;
    type Reader = A::Reader;
    type Writer = A::Writer;
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
        self.check_no_path_collision(path).await?;
        self.inner.write(path, args).await
    }

    async fn copy(&self, from: &str, to: &str, args: OpCopy) -> Result<RpCopy> {
        self.check_no_path_collision(to).await?;
        self.inner.copy(from, to, args).await
    }

    async fn rename(&self, from: &str, to: &str, args: OpRename) -> Result<RpRename> {
        self.check_no_path_collision(to).await?;
        self.inner.rename(from, to, args).await
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
