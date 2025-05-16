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
/// The indentifier is basically the timestamp of the file.
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

use std::{fs::File, io::Write, path::PathBuf};

/// Writes a temp file to disk.
pub(crate) struct InDbTempFileWriter {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    writer_file: File,
    file_path: PathBuf,
    hasher: Hasher,
}

impl InDbTempFileWriter {
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

    /// Create a new BlobsTempFile with random content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub fn random(size_bytes: usize) -> Result<InDbTempFile, std::io::Error> {
        let mut file = Self::new()?;
        let buffer = vec![0u8; size_bytes];
        file.write_chunk(&buffer)?;
        file.complete()
    }

    /// Write a chunk to the file.
    /// Chunk writing is done by the axum body stream and by LMDB itself.
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
            dir: self.dir,
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
pub(crate) struct InDbTempFile {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    file_path: PathBuf,
    file_size: usize,
    file_hash: Hash,
}

impl InDbTempFile {
    /// Create a new BlobsTempFile with random content.
    /// Convenient method used for testing.
    #[cfg(test)]
    pub fn random(size_bytes: usize) -> Result<Self, std::io::Error> {
        InDbTempFileWriter::random(size_bytes)
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
