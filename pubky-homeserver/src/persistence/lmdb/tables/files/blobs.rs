use crate::persistence::files::{FileIoError, FileMetadata, WriteStreamError};

use super::super::super::LmDB;
use super::{AsyncInDbTempFileWriter, InDbFileId, InDbTempFile, SyncInDbTempFileWriter};
use futures_util::{Stream, StreamExt};
use heed::{types::Bytes, Database};
use std::io::Read;

/// (entry timestamp | chunk_index BE) => bytes
pub type BlobsTable = Database<Bytes, Bytes>;
pub const BLOBS_TABLE: &str = "blobs";

impl LmDB {
    /// Read the blobs into a temporary file.
    ///
    /// The file is written to disk to minimize the size/duration of the LMDB transaction.
    pub(crate) fn read_file_sync(&self, id: &InDbFileId) -> Result<InDbTempFile, FileIoError> {
        let mut file_writer = SyncInDbTempFileWriter::new()?;
        let rtxn = self.env.read_txn()?;
        let blobs_iter = self
            .tables
            .blobs
            .prefix_iter(&rtxn, &id.bytes())?
            .map(|i| i.map(|(_, bytes)| bytes));
        let mut file_exists = false;
        for read_result in blobs_iter {
            file_exists = true;
            let chunk = read_result?;
            file_writer.write_chunk(chunk)?;
        }

        if !file_exists {
            return Err(FileIoError::NotFound);
        }

        let file = file_writer.complete()?;
        rtxn.commit()?;
        Ok(file)
    }

    /// Read the blobs into a temporary file asynchronously.
    pub(crate) async fn read_file(&self, id: &InDbFileId) -> Result<InDbTempFile, FileIoError> {
        let db = self.clone();
        let id = *id;
        let join_handle = tokio::task::spawn_blocking(move || -> Result<InDbTempFile, FileIoError> {
            db.read_file_sync(&id)
        })
        .await;
        match join_handle {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Error reading file. JoinError: {:?}", e);
                return Err(FileIoError::NotFound);
            }
        }
    }

    /// Write the blobs from a temporary file to LMDB.
    pub(crate) fn write_file_sync<'txn>(
        &'txn self,
        file: &InDbTempFile,
        wtxn: &mut heed::RwTxn<'txn>,
    ) -> Result<InDbFileId, FileIoError> {
        let id = InDbFileId::new();
        let mut file_handle = file.open_file_handle()?;

        let mut blob_index: u32 = 0;
        loop {
            let mut blob = vec![0_u8; self.max_chunk_size];
            let bytes_read = file_handle.read(&mut blob)?;
            let is_end_of_file = bytes_read == 0;
            if is_end_of_file {
                break; // EOF reached
            }

            let blob_key = id.get_blob_key(blob_index);
            self.tables
                .blobs
                .put(wtxn, &blob_key, &blob[..bytes_read])?;

            blob_index += 1;
        }

        Ok(id)
    }

    /// Write the blobs from a stream to LMDB.
    pub(crate) async fn write_file_from_stream<'txn>(
        &'txn self,
        mut stream: impl Stream<Item = Result<bytes::Bytes, WriteStreamError>> + Unpin + Send,
    ) -> Result<FileMetadata, FileIoError> {
        // First, write the stream to a temporary file using AsyncInDbTempFileWriter
        let mut temp_file_writer = AsyncInDbTempFileWriter::new().await?;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            temp_file_writer.write_chunk(&chunk).await?;
        }

        let temp_file = temp_file_writer.complete().await?;
        let mut metadata = temp_file.metadata().clone();

        // Now write the temporary file to LMDB using the existing sync method
        let mut wtxn = self.env.write_txn()?;
        let file_id = self.write_file_sync(&temp_file, &mut wtxn)?;
        wtxn.commit()?;
        metadata.modified_at = file_id.timestamp().clone();

        Ok(metadata)
    }

    /// Delete the blobs from LMDB.
    pub(crate) fn delete_file<'txn>(
        &'txn self,
        file: &InDbFileId,
        wtxn: &mut heed::RwTxn<'txn>,
    ) -> Result<(), FileIoError> {

        let mut keys = vec![];
        {
            let mut iter = self.tables.blobs.prefix_iter_mut(wtxn, &file.bytes())?;
            while let Some(result) = iter.next() {
                let (key, _) = result?;
                keys.push(key.to_vec());
            }
        }
        if keys.is_empty() {
            return Err(FileIoError::NotFound);
        }
        for key in keys.iter() {
            self.tables.blobs.delete(wtxn, key)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_write_read_delete_file() {
        let lmdb = LmDB::test();

        // Write file to LMDB
        let write_file = InDbTempFile::zeros(50).await.unwrap();
        let mut wtxn = lmdb.env.write_txn().unwrap();
        let id = lmdb.write_file_sync(&write_file, &mut wtxn).unwrap();
        wtxn.commit().unwrap();

        // Read file from LMDB
        let read_file = lmdb.read_file(&id).await.unwrap();

        assert_eq!(read_file.metadata().length, write_file.metadata().length);
        assert_eq!(read_file.metadata().hash, write_file.metadata().hash);

        let written_file_content = std::fs::read(write_file.path()).unwrap();
        let read_file_content = std::fs::read(read_file.path()).unwrap();
        assert_eq!(written_file_content, read_file_content);

        // Delete file from LMDB
        let mut wtxn = lmdb.env.write_txn().unwrap();
        lmdb.delete_file(&id, &mut wtxn).unwrap();
        wtxn.commit().unwrap();

        // Try to read file from LMDB
        match lmdb.read_file(&id).await {
            Ok(_) => {
                panic!("File should be deleted");
            }
            Err(e) => {
                assert_eq!(e.to_string(), FileIoError::NotFound.to_string());
            }
        }
        
    }
}
