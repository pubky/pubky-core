use crate::core::HomeserverCore;
use crate::DataDirTrait;
use crate::{
    admin::run_admin_server, app_context::AppContext, data_directory::DataDir, SignupMode,
};
use anyhow::Result;
use pkarr::PublicKey;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
/// Homeserver Core + I/O (http server and pkarr publishing).
pub struct HomeserverSuite {
    context: AppContext,
    core: HomeserverCore,
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

    /// Run a Homeserver
    ///
    /// # Safety
    /// Homeserver uses LMDB, [opening][heed::EnvOpenOptions::open] which is marked unsafe,
    /// because the possible Undefined Behavior (UB) if the lock file is broken.
    async fn run(context: AppContext) -> Result<Self> {
        tracing::debug!(?context, "Running homeserver with configurations");
        let core = HomeserverCore::new(&context).await?;
        run_admin_server(&context).await?;

        Ok(Self { context, core })
    }

    /// Run a Homeserver with configurations suitable for ephemeral tests.

    pub async fn run_test(bootstrap: &[String]) -> Result<Self> {
        use crate::DomainPort;
        use std::str::FromStr;

        let mut context = AppContext::test();
        context.config_toml.pkdns.dht_bootstrap_nodes = Some(
            bootstrap
                .iter()
                .map(|s| DomainPort::from_str(s).unwrap())
                .collect(),
        );
        Self::run(context).await
    }

    /// Run a Homeserver with configurations suitable for ephemeral tests.
    /// That requires signup tokens.
    pub async fn run_test_with_signup_tokens(bootstrap: &[String]) -> Result<Self> {
        use crate::DomainPort;
        use std::str::FromStr;

        let mut context = AppContext::test();
        context.config_toml.pkdns.dht_bootstrap_nodes = Some(
            bootstrap
                .iter()
                .map(|s| DomainPort::from_str(s).unwrap())
                .collect(),
        );
        context.config_toml.general.signup_mode = SignupMode::TokenRequired;
        Self::run(context).await
    }

    // === Getters ===

    /// Returns the public_key of this server.
    pub fn public_key(&self) -> PublicKey {
        self.context.keypair.public_key()
    }

    /// Returns the `https://<server public key>` url
    pub fn url(&self) -> url::Url {
        url::Url::parse(&format!("https://{}", self.public_key())).expect("valid url")
    }

    // === Public Methods ===

    /// Send a shutdown signal to all open resources
    pub async fn shutdown(&mut self) {
        self.core.stop().await;
    }
}
