use super::super::super::LmDB;
use super::{InDbFileId, InDbTempFile, SyncInDbTempFileWriter};
use heed::{types::Bytes, Database};
use std::io::Read;

/// (entry timestamp | chunk_index BE) => bytes
pub type BlobsTable = Database<Bytes, Bytes>;
pub const BLOBS_TABLE: &str = "blobs";

impl LmDB {
    /// Read the blobs into a temporary file.
    ///
    /// The file is written to disk to minimize the size/duration of the LMDB transaction.
    pub(crate) fn read_file_sync(&self, id: &InDbFileId) -> anyhow::Result<InDbTempFile> {
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
            return Ok(InDbTempFile::empty()?);
        }

        let file = file_writer.complete()?;
        rtxn.commit()?;
        Ok(file)
    }

    /// Read the blobs into a temporary file asynchronously.
    pub(crate) async fn read_file(&self, id: &InDbFileId) -> anyhow::Result<InDbTempFile> {
        let db = self.clone();
        let id = *id;
        let join_handle = tokio::task::spawn_blocking(move || -> anyhow::Result<InDbTempFile> {
            db.read_file_sync(&id)
        })
        .await;
        match join_handle {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Error reading file. JoinError: {:?}", e);
                Err(e.into())
            }
        }
    }

    /// Write the blobs from a temporary file to LMDB.
    pub(crate) fn write_file_sync<'txn>(
        &'txn self,
        file: &InDbTempFile,
        wtxn: &mut heed::RwTxn<'txn>,
    ) -> anyhow::Result<(InDbFileId, String)> {
        let id = InDbFileId::new();
        let mut file_handle = file.open_file_handle()?;

        let mut buffer = [0u8; 512];
        let n = file_handle.read(&mut buffer)?;

        // Run type inference on the buffer slice
        let mime_type = if let Some(kind) = infer::get(&buffer[..n]) {
            kind.mime_type().to_string()
        } else {
            // default MIME type as per
            // https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/MIME_types#application
            "application/octet-stream".to_string()
        };

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

        Ok((id, mime_type))
    }

    /// Delete the blobs from LMDB.
    pub(crate) fn delete_file<'txn>(
        &'txn self,
        file: &InDbFileId,
        wtxn: &mut heed::RwTxn<'txn>,
    ) -> anyhow::Result<bool> {
        let mut deleted_chunks = false;

        {
            let mut iter = self.tables.blobs.prefix_iter_mut(wtxn, &file.bytes())?;

            while iter.next().is_some() {
                unsafe {
                    deleted_chunks = iter.del_current()?;
                }
            }
        }
        Ok(deleted_chunks)
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
        let (id, file_type) = lmdb.write_file_sync(&write_file, &mut wtxn).unwrap();
        assert_eq!(file_type, "application/octet-stream".to_string());
        wtxn.commit().unwrap();

        // Read file from LMDB
        let read_file = lmdb.read_file(&id).await.unwrap();

        assert_eq!(read_file.len(), write_file.len());
        assert_eq!(read_file.hash(), write_file.hash());

        let written_file_content = std::fs::read(write_file.path()).unwrap();
        let read_file_content = std::fs::read(read_file.path()).unwrap();
        assert_eq!(written_file_content, read_file_content);

        // Delete file from LMDB
        let mut wtxn = lmdb.env.write_txn().unwrap();
        let deleted = lmdb.delete_file(&id, &mut wtxn).unwrap();
        wtxn.commit().unwrap();
        assert!(deleted);

        // Try to read file from LMDB
        let read_file = lmdb.read_file(&id).await.unwrap();
        assert_eq!(read_file.len(), 0);
    }

    #[tokio::test]
    async fn test_write_empty_file() {
        let lmdb = LmDB::test();

        // Write file to LMDB
        let write_file = InDbTempFile::empty().unwrap();
        let mut wtxn = lmdb.env.write_txn().unwrap();
        let ( id, file_type ) = lmdb.write_file_sync(&write_file, &mut wtxn).unwrap();
        assert_eq!(file_type, "application/octet-stream".to_string());
        wtxn.commit().unwrap();

        // Read file from LMDB
        let read_file = lmdb.read_file(&id).await.unwrap();

        assert_eq!(read_file.len(), write_file.len());
        assert_eq!(read_file.hash(), write_file.hash());

        let written_file_content = std::fs::read(write_file.path()).unwrap();
        let read_file_content = std::fs::read(read_file.path()).unwrap();
        assert_eq!(written_file_content, read_file_content);
    }

    #[tokio::test]
    async fn test_write_txt_file() {
        let lmdb = LmDB::test();

        // Write file to LMDB
        let write_file = InDbTempFile::empty().unwrap();
        let mut wtxn = lmdb.env.write_txn().unwrap();
        let ( id, file_type ) = lmdb.write_file_sync(&write_file, &mut wtxn).unwrap();
        assert_eq!(file_type, "application/octet-stream".to_string());
        wtxn.commit().unwrap();

        // Read file from LMDB
        let read_file = lmdb.read_file(&id).await.unwrap();

        assert_eq!(read_file.len(), write_file.len());
        assert_eq!(read_file.hash(), write_file.hash());

        let written_file_content = std::fs::read(write_file.path()).unwrap();
        let read_file_content = std::fs::read(read_file.path()).unwrap();
        assert_eq!(written_file_content, read_file_content);
    }
}
