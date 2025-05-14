use std::io::Write;

use axum::body::BodyDataStream;
use pkarr::PublicKey;
use tokio::fs::File;
use crate::persistence::lmdb::LmDB;
use tokio_util::io::ReaderStream;

#[derive(Clone, Debug)]
pub(crate) struct FileService {
    db: LmDB
}

impl FileService {
    pub fn new(db: LmDB) -> Self {
        Self { db }
    }

    pub async fn write_file(&self, public_key: &PublicKey, path: &str, file: File) -> anyhow::Result<()> {
        let rtxn = self.db.env.write_txn()?;
        let entry = self.db.get_entry(&rtxn, public_key, path)?;
        rtxn.commit()?;
        Ok(())
    }

    pub async fn stream_file(&self, public_key: &PublicKey, path: &str) -> anyhow::Result<Option<ReaderStream<File>>> {
        let rtxn = self.db.env.read_txn()?;
        let entry = match self.db.get_entry(&rtxn, public_key, path)? {
            Some(entry) => entry,       
            None => return Ok(None),
        };
        let blob = match self.db.read_blob(&entry).await? {
            Some(blob) => blob,
            None => return Ok(None),
        };

        // Write to file so it can be streamed asynchronously
        let mut temp_file = tempfile::tempfile()?;
        temp_file.write_all(&blob)?;
        temp_file.flush()?;
        let file = tokio::fs::File::from_std(temp_file);


        let stream = ReaderStream::new(file);
        Ok(Some(stream))
    }
}


// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[tokio::test]
//     async fn test_stream_entry() {
//         let db = LmDB::test();
//         let entry_service = FileService::new(db);
//         let public_key = PublicKey::from_str("test").unwrap();
//         let path = "test";
//         let stream = entry_service.stream_entry(&public_key, path).await.unwrap();
//         assert!(stream.is_some());
//     }
// }
