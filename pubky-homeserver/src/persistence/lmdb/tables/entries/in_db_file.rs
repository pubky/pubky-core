//!
//! InDbFile is an abstraction over the way we store blobs/chunks in LMDB.
//! Because the value size of LMDB is limited, we need to store multiple blobs for one file.
//!
//! - `InDbFileId` is the identifier of a file that consists of multiple blobs.
//! - `InDbTempFile` is a helper to read/write a file to/from disk.
//!
use pubky_common::crypto::{Hash, Hasher};
use pubky_common::timestamp::Timestamp;

/// A file identifier for a file stored in LMDB.
/// The identifier is basically the timestamp of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InDbFileId(Timestamp);

impl InDbFileId {
    pub fn new() -> Self {
        Self(Timestamp::now())
    }

    pub fn timestamp(&self) -> &Timestamp {
        &self.0
    }

    pub fn bytes(&self) -> [u8; 8] {
        self.0.to_bytes()
    }

    /// Create a blob key from a timestamp and a blob index.
    /// blob key = (timestamp | blob_index) => bytes.
    /// Max file size is 2^32 blobs.
    pub fn get_blob_key(&self, blob_index: u32) -> [u8; 12] {
        let mut blob_key = [0; 12];
        blob_key[0..8].copy_from_slice(&self.bytes());
        blob_key[8..].copy_from_slice(&blob_index.to_be_bytes());
        blob_key
    }
}

impl From<Timestamp> for InDbFileId {
    fn from(timestamp: Timestamp) -> Self {
        Self(timestamp)
    }
}

use std::sync::Arc;
use std::{fs::File, io::Write, path::PathBuf};
use tokio::fs::File as AsyncFile;
use tokio::io::AsyncWriteExt;
use tokio::task;

/// Writes a temp file to disk asynchronously.
#[derive(Debug)]
pub(crate) struct AsyncInDbTempFileWriter {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    writer_file: AsyncFile,
    file_path: PathBuf,
    hasher: Hasher,
}

impl AsyncInDbTempFileWriter {
    pub async fn new() -> Result<Self, std::io::Error> {
        let dir = task::spawn_blocking(tempfile::tempdir)
            .await
            .map_err(|join_error| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Task join error for tempdir creation: {}", join_error),
                )
            })? // Handles JoinError
            .map_err(|io_error| io_error)?; // Handles the Result from tempfile::tempdir()

        let file_path = dir.path().join("entry.bin");
        let writer_file = AsyncFile::create(file_path.clone()).await?;
        let hasher = Hasher::new();

        Ok(Self {
            dir,
            writer_file,
            file_path,
            hasher,
        })
    }

    /// Create a new BlobsTempFile with zero content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub async fn zeros(size_bytes: usize) -> Result<InDbTempFile, std::io::Error> {
        let mut file = Self::new().await?;
        let buffer = vec![0u8; size_bytes];
        file.write_chunk(&buffer).await?;
        file.complete().await
    }

    /// Write a chunk to the file.
    /// Chunk writing is done by the axum body stream and by LMDB itself.
    pub async fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        self.writer_file.write_all(chunk).await?;
        self.hasher.update(chunk);
        Ok(())
    }

    /// Flush the file to disk.
    /// This completes the writing of the file.
    /// Returns a BlobsTempFile that can be used to read the file.
    pub async fn complete(mut self) -> Result<InDbTempFile, std::io::Error> {
        self.writer_file.flush().await?;
        let hash = self.hasher.finalize();
        let file_size = self.writer_file.metadata().await?.len();
        Ok(InDbTempFile {
            dir: Arc::new(self.dir),
            file_path: self.file_path,
            file_size: file_size as usize,
            file_hash: hash,
        })
    }
}

/// Writes a temp file to disk synchronously.
#[derive(Debug)]
pub(crate) struct SyncInDbTempFileWriter {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    writer_file: File,
    file_path: PathBuf,
    hasher: Hasher,
}

impl SyncInDbTempFileWriter {
    pub fn new() -> Result<Self, std::io::Error> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("entry.bin");
        let writer_file = File::create(file_path.clone())?;
        let hasher = Hasher::new();

        Ok(Self {
            dir,
            writer_file,
            file_path,
            hasher,
        })
    }

    /// Create a new BlobsTempFile with zero content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub fn zeros(size_bytes: usize) -> Result<InDbTempFile, std::io::Error> {
        let mut file = Self::new()?;
        let buffer = vec![0u8; size_bytes];
        file.write_chunk(&buffer)?;
        file.complete()
    }

    /// Write a chunk to the file.
    pub fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        self.writer_file.write_all(chunk)?;
        self.hasher.update(chunk);
        Ok(())
    }

    /// Flush the file to disk.
    /// This completes the writing of the file.
    /// Returns a BlobsTempFile that can be used to read the file.
    pub fn complete(mut self) -> Result<InDbTempFile, std::io::Error> {
        self.writer_file.flush()?;
        let hash = self.hasher.finalize();
        let file_size = self.writer_file.metadata()?.len();
        Ok(InDbTempFile {
            dir: Arc::new(self.dir),
            file_path: self.file_path,
            file_size: file_size as usize,
            file_hash: hash,
        })
    }
}

/// A temporary file helper for Entry.
///
/// Every file in LMDB is first written to disk before being written to LMDB.
/// The same is true if you read a file from LMDB.
///
/// This is to keep the LMDB transaction small and fast.
///
/// As soon as EntryTempFile is dropped, the file on disk is deleted.
///
#[derive(Debug, Clone)]
pub struct InDbTempFile {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: Arc<tempfile::TempDir>,
    file_path: PathBuf,
    file_size: usize,
    file_hash: Hash,
}

impl InDbTempFile {
    /// Create a new BlobsTempFile with random content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub async fn zeros(size_bytes: usize) -> Result<Self, std::io::Error> {
        AsyncInDbTempFileWriter::zeros(size_bytes).await
    }

    /// Create a new InDbTempFile with zero content.
    pub fn empty() -> Result<Self, std::io::Error> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("entry.bin");
        std::fs::File::create(file_path.clone())?;
        let file_size = 0;
        let hasher = Hasher::new();
        let file_hash = hasher.finalize();
        Ok(Self {
            dir: Arc::new(dir),
            file_path,
            file_size,
            file_hash,
        })
    }

    pub fn len(&self) -> usize {
        self.file_size
    }

    pub fn hash(&self) -> &Hash {
        &self.file_hash
    }

    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }

    /// Open the file on disk.
    pub fn open_file_handle(&self) -> Result<File, std::io::Error> {
        File::open(self.file_path.as_path())
    }
}
