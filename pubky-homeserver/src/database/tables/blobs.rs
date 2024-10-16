use heed::{types::Bytes, Database, RoTxn};
use pubky_common::timestamp::Timestamp;

use crate::database::DB;

/// hash of the blob => bytes.
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";

impl DB {
    pub fn get_blob<'txn>(
        &self,
        rtxn: &'txn RoTxn,
        timestamp: &Timestamp,
    ) -> anyhow::Result<impl Iterator<Item = Result<&'txn [u8], heed::Error>> + 'txn> {
        Ok(self
            .tables
            .blobs
            .prefix_iter(rtxn, &timestamp.to_bytes())?
            .map(|i| i.map(|(_, bytes)| bytes)))
    }
}
