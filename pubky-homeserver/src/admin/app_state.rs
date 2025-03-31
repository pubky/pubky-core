use crate::persistence::lmdb::LmDB;

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) db: LmDB,
}

impl AppState {
    pub fn new(db: LmDB) -> Self {
        Self { db }
    }
}
