use crate::admin::{AdminServer, AdminServerBuildError};
use crate::core::{HomeserverBuildError, HomeserverCore};
use crate::{DataDirMock, DataDirTrait};
use crate::{app_context::AppContext, data_directory::DataDir};
use anyhow::Result;
use pkarr::PublicKey;
use std::path::PathBuf;
use std::sync::Arc;

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
    pub async fn run_with_data_dir_path(dir_path: PathBuf) -> Result<Self> {
        let data_dir = DataDir::new(dir_path);
        let context = AppContext::try_from(data_dir)?;
        Self::run(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn run_with_data_dir_trait(dir: Arc<dyn DataDirTrait>) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::run(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn run_with_data_dir(dir: DataDir) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::run(context).await
    }

    /// Run the homeserver with configurations from a data directory mock.
    pub async fn run_with_data_dir_mock(dir: DataDirMock) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::run(context).await
    }

    /// Run a Homeserver
    pub async fn run(context: AppContext) -> Result<Self> {
        let core = HomeserverCore::new(context.clone()).await?;
        let admin_server = AdminServer::run(&context).await?;

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
        url::Url::parse(&format!("pubky://{}", self.public_key())).expect("valid url")
    }

    /// Returns the `https://<server public key>` url
    pub fn icann_http_url(&self) -> url::Url {
        url::Url::parse(&self.core.icann_http_url()).expect("valid url")
    }
}


