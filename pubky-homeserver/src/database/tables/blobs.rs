use heed::{types::Bytes, Database};
use pkarr::PublicKey;

use crate::database::DB;

use super::entries::Entry;

/// hash of the blob => bytes.
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";

impl DB {
    pub fn get_blob(
        &self,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<Option<bytes::Bytes>> {
        let rtxn = self.env.read_txn()?;

        let key = format!("{public_key}/{path}");

        let result = if let Some(bytes) = self.tables.entries.get(&rtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            self.tables
                .blobs
                .get(&rtxn, entry.content_hash())?
                .map(|blob| bytes::Bytes::from(blob[8..].to_vec()))
        } else {
            None
        };

        rtxn.commit()?;

        Ok(result)
    }
}
