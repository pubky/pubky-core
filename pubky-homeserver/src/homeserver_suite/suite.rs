use crate::admin::{AdminServer, AdminServerBuildError};
use crate::core::{HomeserverBuildError, HomeserverCore};
use crate::tracing::init_tracing_logs_with_config_if_set;
#[cfg(any(test, feature = "testing"))]
use crate::MockDataDir;
use crate::{app_context::AppContext, data_directory::PersistentDataDir};
use anyhow::Result;
use pkarr::PublicKey;
use std::path::PathBuf;

/// Errors that can occur when building a `HomeserverSuite`.
#[derive(thiserror::Error, Debug)]
pub enum HomeserverSuiteBuildError {
    /// Failed to build the homeserver.
    #[error("Failed to build homeserver: {0}")]
    Homeserver(HomeserverBuildError),
    /// Failed to build the admin server.
    #[error("Failed to build admin server: {0}")]
    Admin(AdminServerBuildError),
}

/// Homeserver with all bells and whistles.
/// Core + Admin server.
///
/// When dropped, the homeserver will stop.
pub struct HomeserverSuite {
    context: AppContext,
    #[allow(dead_code)] // Keep this alive. When dropped, the homeserver will stop.
    core: HomeserverCore,
    #[allow(dead_code)] // Keep this alive. When dropped, the admin server will stop.
    admin_server: AdminServer,
}

impl HomeserverSuite {
    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir_path(dir_path: PathBuf) -> Result<Self> {
        let data_dir = PersistentDataDir::new(dir_path);
        let context = AppContext::read_from(data_dir).await?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir(dir: PersistentDataDir) -> Result<Self> {
        let context = AppContext::read_from(dir).await?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory mock.
    #[cfg(any(test, feature = "testing"))]
    pub async fn start_with_mock_data_dir(dir: MockDataDir) -> Result<Self> {
        let context = AppContext::read_from(dir).await?;
        Self::start(context).await
    }

    /// Run a Homeserver
    pub async fn start(context: AppContext) -> Result<Self> {
        // Tracing Subscriber initialization based on the config file.
        let _ = init_tracing_logs_with_config_if_set(&context.config_toml);

        tracing::debug!("Homeserver data dir: {}", context.data_dir.path().display());

        let core = HomeserverCore::new(context.clone()).await?;
        let admin_server = AdminServer::start(&context).await?;

        Ok(Self {
            context,
            core,
            admin_server,
        })
    }

    /// Get the core of the homeserver suite.
    pub fn core(&self) -> &HomeserverCore {
        &self.core
    }

    /// Get the admin server of the homeserver suite.
    pub fn admin(&self) -> &AdminServer {
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
        url::Url::parse(&self.core.icann_http_url()).expect("valid url")
    }
}
