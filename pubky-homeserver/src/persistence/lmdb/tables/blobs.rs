use heed::{types::Bytes, Database, RoTxn};

use super::super::LmDB;

use super::entries::Entry;

/// (entry timestamp | chunk_index BE) => bytes
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";

impl LmDB {
    pub fn read_entry_content<'txn>(
        &self,
        rtxn: &'txn RoTxn,
        entry: &Entry,
    ) -> anyhow::Result<impl Iterator<Item = Result<&'txn [u8], heed::Error>> + 'txn> {
        Ok(self
            .tables
            .blobs
            .prefix_iter(rtxn, &entry.timestamp().to_bytes())?
            .map(|i| i.map(|(_, bytes)| bytes)))
    }

    pub async fn read_blob(&self, entry: &Entry) -> anyhow::Result<Option<Vec<u8>>> {
        // spawn blocking
        let blob_key = entry.timestamp().to_bytes();
        let rtxn = self.env.read_txn()?;
        let blob = self.tables.blobs.get(&rtxn, &blob_key)?.map(|b| b.to_vec());
        rtxn.commit()?;
        Ok(blob)
    }
}
