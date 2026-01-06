use dav_server::{fakels::FakeLs, DavHandler};
use dav_server_opendalfs::OpendalFs;

use crate::persistence::{files::FileService, sql::SqlDb};
use crate::ConfigToml;

#[derive(Clone, Default)]
pub(crate) struct AdminMetadata {
    pub(crate) public_key: String,
    pub(crate) pkarr_pubky_address: Option<String>,
    pub(crate) pkarr_icann_domain: Option<String>,
    pub(crate) version: String,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) sql_db: SqlDb,
    pub(crate) file_service: FileService,
    pub(crate) admin_password: String,
    pub(crate) inner_dav_handler: DavHandler,
    pub(crate) metadata: AdminMetadata,
}

impl AppState {
    pub fn new(sql_db: SqlDb, file_service: FileService, admin_password: &str) -> Self {
        let webdavfs = OpendalFs::new(file_service.opendal.operator.clone());
        let inner_dav_handler = DavHandler::builder()
            .filesystem(webdavfs)
            .locksystem(FakeLs::new())
            .strip_prefix("/dav")
            .autoindex(true)
            .build_handler();
        Self {
            sql_db,
            file_service,
            admin_password: admin_password.to_string(),
            inner_dav_handler,
            metadata: AdminMetadata::default(),
        }
    }

    pub fn with_metadata_from_config(
        mut self,
        public_key: String,
        config: &ConfigToml,
        version: &str,
    ) -> Self {
        self.metadata = AdminMetadata {
            public_key,
            pkarr_pubky_address: pkarr_pubky_tls_address(config),
            pkarr_icann_domain: pkarr_icann_domain(config),
            version: version.to_string(),
        };
        self
    }
}

fn pkarr_pubky_tls_address(config: &ConfigToml) -> Option<String> {
    let port = config
        .pkdns
        .public_pubky_tls_port
        .unwrap_or(config.drive.pubky_listen_socket.port());

    if port == 0 {
        return None;
    }

    Some(format!("{}:{}", config.pkdns.public_ip, port))
}

fn pkarr_icann_domain(config: &ConfigToml) -> Option<String> {
    let domain = config.pkdns.icann_domain.as_ref()?;
    let port = config
        .pkdns
        .public_icann_http_port
        .unwrap_or(config.drive.icann_listen_socket.port());

    if port == 0 {
        return None;
    }

    Some(format!("{}:{}", domain.0, port))
}
