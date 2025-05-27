use crate::admin::{AdminServer, AdminServerBuildError};
use crate::core::{HomeserverBuildError, HomeserverCore};
use crate::{app_context::AppContext, data_directory::PersistentDataDir};
use crate::MockDataDir;
use anyhow::Result;
use pkarr::PublicKey;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

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
        let context = AppContext::try_from(data_dir)?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory.
    pub async fn start_with_persistent_data_dir(dir: PersistentDataDir) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::start(context).await
    }

    /// Run the homeserver with configurations from a data directory mock.
    pub async fn start_with_mock_data_dir(dir: MockDataDir) -> Result<Self> {
        let context = AppContext::try_from(dir)?;
        Self::start(context).await
    }

    /// Run a Homeserver
    pub async fn start(context: AppContext) -> Result<Self> {
        let env_filter = match EnvFilter::try_from_default_env() {
            Ok(f) => f,
            Err(_) => {
                // create from configuration
                let mut filter = EnvFilter::new("");
                match context.config_toml.logging {
                    Some(ref config) => {
                        filter = filter.add_directive(config.level.to_owned().into());
                        // Add any specific filters
                        for filter_str in &config.filters {
                            filter = filter.add_directive(filter_str.to_owned().into());
                        }
                    }
                    _ => {}
                }
                filter
            }
        };

        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init()
            .map_err(|_| {
                tracing::debug!(
                    "Instance {} trace config will be ignored",
                    &context.keypair.public_key()
                )
            });

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
