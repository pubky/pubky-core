use dav_server::DavHandler;
use dav_server_opendalfs::OpendalFs;

use crate::persistence::{files::FileService, lmdb::LmDB, sql::SqlDb};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) db: LmDB,
    pub(crate) sql_db: SqlDb,
    pub(crate) file_service: FileService,
    pub(crate) admin_password: String,
    pub(crate) inner_dav_handler: DavHandler,
}

impl AppState {
    pub fn new(db: LmDB, sql_db: SqlDb, file_service: FileService, admin_password: &str) -> Self {
        let webdavfs = OpendalFs::new(file_service.opendal.operator.clone());
        let inner_dav_handler = DavHandler::builder()
            .filesystem(webdavfs)
            .strip_prefix("/dav")
            .autoindex(true)
            .build_handler();
        Self {
            db,
            sql_db,
            file_service,
            admin_password: admin_password.to_string(),
            inner_dav_handler,
        }
    }
}
