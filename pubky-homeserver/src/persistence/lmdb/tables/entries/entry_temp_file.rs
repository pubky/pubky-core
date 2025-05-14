use std::{fs::File, io::Write, path::PathBuf};


/// A temporary file helper for Entry.
/// 
/// Every file in LMDB is first written to disk before being written to LMDB.
/// The same is true if you read a file from LMDB.
/// 
/// This is to keep the LMDB transaction small and fast.
/// 
/// As soon as EntryTempFile is dropped, the file on disk is deleted.
pub (crate) struct EntryTempFile {
    // Temp dir is automatically deleted when the EntryTempFile is dropped.
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    is_flushed: bool,
    writer_file: File,
    file_path: PathBuf,
}

impl EntryTempFile {
    /// Create a new EntryTempFile.
    pub fn new() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let file_path = dir.path().join("entry.bin");
        let writer_file = File::create(file_path.clone())?;

        Ok(Self { dir, writer_file, file_path, is_flushed: false })
    }

    /// Write a chunk to the file.
    /// Chunk writing is done by the axum body stream and by LMDB itself.
    pub fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), std::io::Error> {
        self.writer_file.write_all(chunk)?;
        Ok(())
    }

    /// Flush the file to disk.
    /// This completes the writing of the file.
    /// After this call, the file can only be read.
    pub fn flush(&mut self) -> Result<(), std::io::Error> {
        self.writer_file.flush()?;
        self.is_flushed = true;
        Ok(())
    }

    /// Open the file on disk.
    /// Important: This will flush the file to disk if it is not already flushed.
    pub fn open_file(&mut self) -> Result<File, std::io::Error> {
        if !self.is_flushed {
            self.flush()?;
        }

        File::open(self.file_path.clone())
    }
}
