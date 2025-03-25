use crate::persistence::lmdb::LmDB;


#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) db: LmDB,
    pub(crate) password: String,
}

impl AppState {
    pub fn new(db: LmDB, password: String) -> Self {
        Self { db, password }
    }
}