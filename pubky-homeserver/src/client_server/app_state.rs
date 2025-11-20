use crate::persistence::files::FileService;
use crate::persistence::sql::SqlDb;
use dav_server::DavHandler;
use dav_server_opendalfs::OpendalFs;

use crate::SignupMode;
use pubky_common::auth::AuthVerifier;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) verifier: AuthVerifier,
    /// The SQL database connection.
    pub(crate) sql_db: SqlDb,
    pub(crate) file_service: FileService,
    pub(crate) signup_mode: SignupMode,
    /// If `Some(bytes)` the quota is enforced, else unlimited.
    pub(crate) user_quota_bytes: Option<u64>,
    pub(crate) inner_dav_handler: DavHandler,
}

impl AppState {
    pub fn new(
        verifier: AuthVerifier,
        sql_db: SqlDb,
        file_service: FileService,
        signup_mode: SignupMode,
        user_quota_bytes: Option<u64>,
    ) -> Self {
        // TODO: allow db lookup for json content as well?

        let webdavfs = OpendalFs::new(file_service.opendal.operator.clone());
        let inner_dav_handler = DavHandler::builder()
            .filesystem(webdavfs)
            .strip_prefix("/dav")
            .autoindex(true)
            .build_handler();

        Self {
            verifier,
            sql_db,
            file_service,
            signup_mode,
            user_quota_bytes,
            inner_dav_handler,
        }
    }
}
