use crate::persistence::{files::FileService, lmdb::LmDB};

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub(crate) db: LmDB,
    pub(crate) file_service: FileService,
}

impl AppState {
    pub fn new(db: LmDB, file_service: FileService) -> Self {
        Self { db, file_service }
    }
}
