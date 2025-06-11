//!
//! InDbFile is an abstraction over the way we store blobs/chunks in LMDB.
//! Because the value size of LMDB is limited, we need to store multiple blobs for one file.
//!
//! - `InDbFileId` is the identifier of a file that consists of multiple blobs.
//! - `InDbTempFile` is a helper to read/write a file to/from disk.
//!
use pubky_common::timestamp::Timestamp;
use std::sync::Arc;
use std::{fs::File, io::Write, path::PathBuf};
use tokio::fs::File as AsyncFile;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;

use crate::persistence::files::FileStream;
use crate::persistence::files::{FileMetadata, FileMetadataBuilder};

/// A file identifier for a file stored in LMDB.
/// The identifier is basically the timestamp of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InDbFileId(pub Timestamp);

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

impl Default for InDbFileId {
    fn default() -> Self {
        Self::new()
    }
}

/// Writes a temp file to disk asynchronously.
#[derive(Debug)]
pub(crate) struct AsyncInDbTempFileWriter {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    writer_file: AsyncFile,
    file_path: PathBuf,
    metadata: FileMetadataBuilder,
}

impl AsyncInDbTempFileWriter {
    pub async fn new() -> Result<Self, std::io::Error> {
        let dir = tempfile::tempdir()?;

        let file_path = dir.path().join("entry.bin");
        let writer_file = AsyncFile::create(file_path.clone()).await?;

        Ok(Self {
            dir,
            writer_file,
            file_path,
            metadata: FileMetadataBuilder::default(),
        })
    }

    /// Create a new BlobsTempFile with zero content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub async fn zeros(size_bytes: usize) -> Result<InDbTempFile, std::io::Error> {
        let mut writer = Self::new().await?;
        let buffer = vec![0u8; size_bytes];
        writer.write_chunk(&buffer).await?;
        writer.complete().await
    }

    #[cfg(test)]
    pub async fn png_pixel() -> Result<InDbTempFile, std::io::Error> {
        let mut file = Self::new().await?;
        let png_magic_bytes: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        file.write_chunk(&png_magic_bytes).await?;
        file.guess_mime_type_from_path("test.png");
        file.complete().await
    }

    /// Write a chunk to the file.
    /// Chunk writing is done by the axum body stream and by LMDB itself.
    pub async fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        self.writer_file.write_all(chunk).await?;
        self.metadata.update(chunk);
        Ok(())
    }

    /// If a path is provided it can be used to guess the content type.
    /// This is useful in case the magic bytes are not enough to determine the content type.
    pub fn guess_mime_type_from_path(&mut self, path: &str) {
        self.metadata.guess_mime_type_from_path(path);
    }

    /// Flush the file to disk.
    /// This completes the writing of the file.
    /// Returns a BlobsTempFile that can be used to read the file.
    pub async fn complete(mut self) -> Result<InDbTempFile, std::io::Error> {
        self.writer_file.flush().await?;
        let metadata = self.metadata.finalize();
        Ok(InDbTempFile {
            dir: Arc::new(self.dir),
            file_path: self.file_path,
            metadata,
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
    metadata: FileMetadataBuilder,
}

impl SyncInDbTempFileWriter {
    pub fn new() -> Result<Self, std::io::Error> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("entry.bin");
        let writer_file = File::create(file_path.clone())?;

        Ok(Self {
            dir,
            writer_file,
            file_path,
            metadata: FileMetadataBuilder::default(),
        })
    }

    /// Write a chunk to the file.
    pub fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        self.writer_file.write_all(chunk)?;
        self.metadata.update(chunk);
        Ok(())
    }

    /// Flush the file to disk.
    /// This completes the writing of the file.
    /// Returns a BlobsTempFile that can be used to read the file.
    pub fn complete(mut self) -> Result<InDbTempFile, std::io::Error> {
        self.writer_file.flush()?;
        let metadata = self.metadata.finalize();
        Ok(InDbTempFile {
            dir: Arc::new(self.dir),
            file_path: self.file_path,
            metadata,
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
    metadata: FileMetadata,
}

impl InDbTempFile {
    /// Create a new BlobsTempFile with random content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub async fn zeros(size_bytes: usize) -> Result<Self, std::io::Error> {
        AsyncInDbTempFileWriter::zeros(size_bytes).await
    }

    pub fn metadata(&self) -> &FileMetadata {
        &self.metadata
    }

    #[cfg(test)]
    pub async fn png_pixel() -> Result<Self, std::io::Error> {
        AsyncInDbTempFileWriter::png_pixel().await
    }

    /// Get the path of the file on disk.
    #[cfg(test)]
    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }

    /// Open the file on disk.
    pub fn open_file_handle(&self) -> Result<File, std::io::Error> {
        File::open(self.file_path.as_path())
    }

    pub fn as_stream(&self) -> Result<FileStream, std::io::Error> {
        let file = std::fs::File::open(&self.file_path)?;
        let async_file = tokio::fs::File::from_std(file);
        let stream = ReaderStream::new(async_file);
        Ok(Box::new(stream))
    }
}
