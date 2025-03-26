use pkarr::Keypair;

use crate::{persistence::lmdb::LmDB, ConfigToml, DataDir};

/// The application context shared between all components.
/// 
/// Created by a `DataDir` instance.
/// `AppContext::try_from(data_dir)`
#[derive(Debug, Clone)]
pub(crate) struct AppContext {
    /// A list of all shared resources.
    pub(crate) db: LmDB,
    pub(crate) config_toml: ConfigToml,
    pub(crate) data_dir: DataDir,
    pub(crate) keypair: Keypair,
    /// Main pkarr client instance.
    pub(crate) pkarr_client: pkarr::Client,
    /// pkarr client builder in case we need to create a more instances.
    pub(crate) pkarr_builder: pkarr::ClientBuilder,
}

impl AppContext {
    #[cfg(test)]
    pub fn test() -> Self {
        use crate::DataDir;
        let data_dir = DataDir::test();
        Self::try_from(data_dir).unwrap()
    }
}

impl TryFrom<DataDir> for AppContext {
    type Error = anyhow::Error;

    fn try_from(dir: DataDir) -> Result<Self, Self::Error> {
        dir.ensure_data_dir_exists_and_is_writable()?;
        let conf = dir.read_or_create_config_file()?;
        let keypair = dir.read_or_create_keypair()?;

        let db_path = dir.path().join("data/lmdb");
        let pkarr_builder = Self::build_pkarr_builder_from_config(&conf);
        Ok(Self {
            db: unsafe { LmDB::open(db_path)? },
            pkarr_client: pkarr_builder.clone().build()?,
            pkarr_builder: pkarr_builder,
            config_toml: conf,
            keypair,
            data_dir: dir,

        })
    }
}

impl AppContext {
    /// Build the pkarr client builder based on the config.
    fn build_pkarr_builder_from_config(config_toml: &ConfigToml) -> pkarr::ClientBuilder {
        let mut builder = pkarr::ClientBuilder::default();
        if let Some(bootstrap_nodes) = &config_toml.pkdns.dht_bootstrap_nodes {
            let nodes = bootstrap_nodes
                .iter()
                .map(|node| node.to_string())
                .collect::<Vec<String>>();
            builder.bootstrap(&nodes);
        }
        if let Some(relays) = &config_toml.pkdns.dht_relay_nodes {
            builder.relays(relays);
        }
        if let Some(request_timeout) = &config_toml.pkdns.dht_request_timeout {
            builder.request_timeout(request_timeout.clone());
        }
        builder
    }
}
