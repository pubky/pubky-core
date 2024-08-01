use std::{borrow::Cow, time::SystemTime};

use heed::{
    types::{Bytes, Str},
    BoxedError, BytesDecode, BytesEncode, Database,
};
use pkarr::PublicKey;

use crate::database::DB;

use super::entries::Entry;

/// hash of the blob => bytes.
pub type BlobsTable = Database<Bytes, Bytes>;

pub const BLOBS_TABLE: &str = "blobs";

impl DB {
    pub fn get_blob(
        &mut self,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<Option<bytes::Bytes>> {
        let rtxn = self.env.read_txn()?;

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

        let result = if let Some(bytes) = self.tables.entries.get(&rtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            self.tables
                .blobs
                .get(&rtxn, entry.content_hash())?
                .map(|blob| bytes::Bytes::from(blob.to_vec()))
        } else {
            None
        };

        rtxn.commit();

        Ok(result)
    }
}
