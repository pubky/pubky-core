use std::fs;
use std::path::Path;

use bytes::Bytes;
use heed::{types::Str, Database, Env, EnvOpenOptions, RwTxn};

mod migrations;
pub mod tables;

use pubky_common::crypto::Hasher;

use tables::{entries::Entry, Tables, TABLES_COUNT};

use pkarr::PublicKey;
use tables::blobs::{BlobsTable, BLOBS_TABLE};

#[derive(Debug, Clone)]
pub struct DB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
}

impl DB {
    pub fn open(storage: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(storage).unwrap();

        let env = unsafe { EnvOpenOptions::new().max_dbs(TABLES_COUNT).open(storage) }?;

        let tables = migrations::run(&env)?;

        let db = DB { env, tables };

        Ok(db)
    }

    pub fn put_entry(
        &mut self,
        public_key: &PublicKey,
        path: &str,
        rx: flume::Receiver<Bytes>,
    ) -> anyhow::Result<()> {
        let mut wtxn = self.env.write_txn()?;

        let mut hasher = Hasher::new();
        let mut bytes = vec![];
        let mut length = 0;

        while let Ok(chunk) = rx.recv() {
            hasher.update(&chunk);
            bytes.extend_from_slice(&chunk);
            length += chunk.len();
        }

        let hash = hasher.finalize();

        self.tables.blobs.put(&mut wtxn, hash.as_bytes(), &bytes)?;

        let mut entry = Entry::new();

        entry.set_content_hash(hash);
        entry.set_content_length(length);

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

        self.tables.entries.put(&mut wtxn, &key, &entry.serialize());

        wtxn.commit()?;

        Ok(())
    }

    pub fn get_blob(
        &mut self,
        public_key: &PublicKey,
        path: &str,
    ) -> anyhow::Result<Option<Bytes>> {
        let mut rtxn = self.env.read_txn()?;

        let mut key = vec![];
        key.extend_from_slice(public_key.as_bytes());
        key.extend_from_slice(path.as_bytes());

        if let Some(bytes) = self.tables.entries.get(&rtxn, &key)? {
            let entry = Entry::deserialize(bytes)?;

            if let Some(blob) = self.tables.blobs.get(&rtxn, entry.content_hash())? {
                return Ok(Some(Bytes::from(blob.to_vec())));
            };
        };

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;
    use pubky_common::timestamp::Timestamp;

    use crate::config::Config;

    use super::{Bytes, DB};

    #[tokio::test]
    async fn entries() {
        let storage = std::env::temp_dir()
            .join(Timestamp::now().to_string())
            .join("pubky");

        let mut db = DB::open(&storage).unwrap();

        let keypair = Keypair::random();
        let path = "/pub/foo.txt";

        let (tx, rx) = flume::bounded::<Bytes>(0);

        let mut cloned = db.clone();
        let cloned_keypair = keypair.clone();

        let done = tokio::task::spawn_blocking(move || {
            cloned.put_entry(&cloned_keypair.public_key(), path, rx);
        });

        tx.send(vec![1, 2, 3, 4, 5].into());
        drop(tx);

        done.await;

        let blob = db.get_blob(&keypair.public_key(), path).unwrap().unwrap();

        assert_eq!(blob, Bytes::from(vec![1, 2, 3, 4, 5]));
    }
}
