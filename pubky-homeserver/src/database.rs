use std::fs;

use heed::{Env, EnvOpenOptions};

mod migrations;
pub mod tables;

use crate::config::Config;

use tables::{Tables, TABLES_COUNT};

pub const DEFAULT_MAP_SIZE: usize = 10995116277760; // 10TB (not = disk-space used)

#[derive(Debug, Clone)]
pub struct DB {
    pub(crate) env: Env,
    pub(crate) tables: Tables,
    pub(crate) config: Config,
}

impl DB {
    pub fn open(config: Config) -> anyhow::Result<Self> {
        let storage = config.storage()?;

        fs::create_dir_all(&storage).unwrap();

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(TABLES_COUNT)
                // TODO: Add a configuration option?
                .map_size(DEFAULT_MAP_SIZE)
                .open(storage)
        }?;

        let tables = migrations::run(&env)?;

        let db = DB {
            env,
            tables,
            config,
        };

        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use pkarr::{mainline::Testnet, Keypair};

    use crate::config::Config;

    use super::DB;

    #[tokio::test]
    async fn entries() {
        let db = DB::open(Config::test(&Testnet::new(0))).unwrap();

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
