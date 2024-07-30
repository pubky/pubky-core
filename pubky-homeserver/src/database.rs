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
