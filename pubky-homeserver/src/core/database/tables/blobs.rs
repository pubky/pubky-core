use heed::{types::Bytes, Database, RoTxn};

use crate::core::database::DB;

use super::entries::Entry;

/// (entry timestamp | chunk_index BE) => bytes
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";

impl DB {
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
}
