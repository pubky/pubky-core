use std::fs;

use std::path::Path;

use heed::{Env, EnvOpenOptions};

mod migrations;
pub mod tables;

use tables::{Tables, TABLES_COUNT};

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
    use bytes::Bytes;
    use pkarr::Keypair;
    use pubky_common::timestamp::Timestamp;

    use super::DB;

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
            cloned
                .put_entry(&cloned_keypair.public_key(), path, rx)
                .unwrap();
        });

        tx.send(vec![1, 2, 3, 4, 5].into()).unwrap();
        drop(tx);

        done.await.unwrap();

        let blob = db.get_blob(&keypair.public_key(), path).unwrap().unwrap();

        assert_eq!(blob, Bytes::from(vec![1, 2, 3, 4, 5]));
    }
}
