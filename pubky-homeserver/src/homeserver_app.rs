use crate::admin_server::{AdminServer, AdminServerBuildError};
use crate::client_server::{ClientServer, ClientServerBuildError};
use crate::data_directory::periodic_backup::PeriodicBackup;
use crate::republishers::{
    HomeserverKeyRepublisher, KeyRepublisherBuildError, UserKeysRepublisher,
};
use crate::tracing::init_tracing_logs_with_config_if_set;
#[cfg(any(test, feature = "testing"))]
use crate::MockDataDir;
use crate::{app_context::AppContext, data_directory::PersistentDataDir};
use anyhow::Result;
use pkarr::PublicKey;
use std::path::PathBuf;
use std::time::Duration;

const INITIAL_DELAY_BEFORE_REPUBLISH: Duration = Duration::from_secs(60);

/// Errors that can occur when building a `HomeserverApp`.
#[derive(thiserror::Error, Debug)]
pub enum HomeserverAppBuildError {
    /// Failed to build the homeserver.
    #[error("Failed to build homeserver: {0}")]
    Homeserver(ClientServerBuildError),
    /// Failed to build the admin server.
    #[error("Failed to build admin server: {0}")]
    Admin(AdminServerBuildError),
}

/// Homeserver with all bells and whistles.
/// Core + Admin server.
///
/// When dropped, the homeserver will stop.
pub struct HomeserverApp {
    context: AppContext,

    #[allow(dead_code)] // Keep this alive. When dropped, the homeserver will stop.
    client_server: ClientServer,

    #[allow(dead_code)]
    // Keep this alive. Republishing is stopped when the UserKeysRepublisher is dropped.
    pub(crate) user_keys_republisher: UserKeysRepublisher,

    #[allow(dead_code)]
    // Keep this alive. Republishing is stopped when the HomeserverKeyRepublisher is dropped.
    pub(crate) key_republisher: HomeserverKeyRepublisher,

    #[allow(dead_code)] // Keep this alive. Backup is stopped when the PeriodicBackup is dropped.
    pub(crate) periodic_backup: PeriodicBackup,

    #[allow(dead_code)] // Keep this alive. When dropped, the admin server will stop.
    admin_server: AdminServer,
}

impl HomeserverApp {
    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir_path(dir_path: PathBuf) -> Result<Self> {
        let data_dir = PersistentDataDir::new(dir_path);
        let context = AppContext::try_from(data_dir)?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir(dir: PersistentDataDir) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory mock.
    #[cfg(any(test, feature = "testing"))]
    pub async fn start_with_mock_data_dir(dir: MockDataDir) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::start(context).await
    }

    /// Run a Homeserver
    pub async fn start(context: AppContext) -> Result<Self> {
        // Tracing Subscriber initialization based on the config file.
        let _ = init_tracing_logs_with_config_if_set(&context.config_toml);

        tracing::debug!("Homeserver data dir: {}", context.data_dir.path().display());

        let user_keys_republisher =
            UserKeysRepublisher::start_delayed(&context, INITIAL_DELAY_BEFORE_REPUBLISH);

        let periodic_backup = PeriodicBackup::start(&context);

        let admin_server = AdminServer::start(&context).await?;
        let client_server = ClientServer::start(context.clone()).await?;

        let key_republisher = HomeserverKeyRepublisher::start(
            &context,
            client_server.icann_http_socket.port(),
            client_server.pubky_tls_socket.port(),
        )
        .await
        .map_err(KeyRepublisherBuildError::KeyRepublisher)?;

        Ok(Self {
            context,
            periodic_backup,
            client_server,
            admin_server,
            user_keys_republisher,
            key_republisher,
        })
    }

    /// Get the core of the homeserver app.
    pub fn client_server(&self) -> &ClientServer {
        &self.client_server
    }

    /// Get the admin server of the homeserver app.
    pub fn admin_server(&self) -> &AdminServer {
        &self.admin_server
    }

    /// Returns the public_key of this server.
    pub fn public_key(&self) -> PublicKey {
        self.context.keypair.public_key()
    }

    /// Returns the `https://<server public key>` url
    pub fn pubky_url(&self) -> url::Url {
        url::Url::parse(&format!("https://{}", self.public_key())).expect("valid url")
    }

    /// Returns the `https://<server public key>` url
    pub fn icann_http_url(&self) -> url::Url {
        url::Url::parse(&self.client_server.icann_http_url_string()).expect("valid url")
    }
}
