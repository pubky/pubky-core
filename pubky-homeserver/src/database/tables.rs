pub mod blobs;
pub mod entries;
pub mod sessions;
pub mod users;

use heed::{Env, RwTxn};

use blobs::{BlobsTable, BLOBS_TABLE};
use entries::{EntriesTable, ENTRIES_TABLE};

pub const TABLES_COUNT: u32 = 4;

#[derive(Debug, Clone)]
pub struct Tables {
    pub blobs: BlobsTable,
    pub entries: EntriesTable,
}

impl Tables {
    pub fn new(env: &Env, wtxn: &mut RwTxn) -> anyhow::Result<Self> {
        Ok(Self {
            blobs: env
                .open_database(wtxn, Some(BLOBS_TABLE))?
                .expect("Blobs table already created"),
            entries: env
                .open_database(wtxn, Some(ENTRIES_TABLE))?
                .expect("Entries table already created"),
        })
    }
}
