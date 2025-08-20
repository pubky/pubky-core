//!
//! The application context shared between all components.
//! Think of it as a simple Dependency Injection container.
//!
//! Create with a `DataDir` instance: `AppContext::try_from(data_dir)`
//!

#[cfg(any(test, feature = "testing"))]
use crate::MockDataDir;
use crate::{
    persistence::{
        files::{FileIoError, FileService},
        lmdb::LmDB,
        sql::SqlDb,
    },
    ConfigToml, DataDir
};
use pkarr::Keypair;
use std::{sync::Arc, time::Duration};

/// Errors that can occur when converting a `DataDir` to an `AppContext`.
#[derive(Debug, thiserror::Error)]
pub enum AppContextConversionError {
    /// Failed to ensure data directory exists and is writable.
    #[error("Failed to ensure data directory exists and is writable: {0}")]
    DataDir(anyhow::Error),
    /// Failed to read or create config file.
    #[error("Failed to read or create config file: {0}")]
    Config(anyhow::Error),
    /// Failed to read or create keypair.
    #[error("Failed to read or create keypair: {0}")]
    Keypair(anyhow::Error),
    /// Failed to open LMDB.
    #[error("Failed to open LMDB: {0}")]
    LmDB(anyhow::Error),
    /// Failed to open SQL DB.
    #[error("Failed to open SQL DB: {0}")]
    SqlDb(sqlx::Error),
    /// Failed to build storage operator.
    #[error("Failed to build storage operator: {0}")]
    Storage(FileIoError),
    /// Failed to build pkarr client.
    #[error("Failed to build pkarr client: {0}")]
    Pkarr(pkarr::errors::BuildError),
}

/// The application context shared between all components.
/// Think of it as a simple Dependency Injection container.
///
/// Create with a `DataDir` instance: `AppContext::try_from(data_dir)`
///
#[derive(Clone)]
pub struct AppContext {
    /// A list of all shared resources.
    pub(crate) db: LmDB,
    /// The SQL database connection.
    pub(crate) sql_db: SqlDb,
    /// The storage operator to store files.
    pub(crate) file_service: FileService,
    pub(crate) config_toml: ConfigToml,
    /// Keep data_dir alive. The mock dir will cleanup on drop.
    pub(crate) data_dir: Arc<dyn DataDir>,
    pub(crate) keypair: Keypair,
    /// Main pkarr instance. This will automatically turn into a DHT server after 15 minutes after startup.
    /// We need to keep this alive.
    pub(crate) pkarr_client: pkarr::Client,
    /// pkarr client builder in case we need to create a more instances.
    /// Comes ready with the correct bootstrap nodes and relays.
    pub(crate) pkarr_builder: pkarr::ClientBuilder,
}

impl AppContext {
    /// Create a new AppContext for testing.
    #[cfg(any(test, feature = "testing"))]
    pub async fn test() -> Self {
        let data_dir = MockDataDir::test();
        Self::read_from(data_dir).await.expect("failed to build AppContext from DataDirMock")
    }

    /// Create a new AppContext from a data directory.
    pub async fn read_from<D: DataDir + 'static>(dir: D) -> Result<Self, AppContextConversionError> {
        dir.ensure_data_dir_exists_and_is_writable()
            .map_err(AppContextConversionError::DataDir)?;
        let conf = dir
            .read_or_create_config_file()
            .map_err(AppContextConversionError::Config)?;
        let keypair = dir
            .read_or_create_keypair()
            .map_err(AppContextConversionError::Keypair)?;

        let db_path = dir.path().join("data/lmdb");
        let db = unsafe { LmDB::open(&db_path).map_err(AppContextConversionError::LmDB)? };
        let file_service = FileService::new_from_config(&conf, dir.path(), db.clone())
            .map_err(AppContextConversionError::Storage)?;
        let pkarr_builder = Self::build_pkarr_builder_from_config(&conf);

        let sql_db = Self::connect_to_sql_db(&conf).await?;

        Ok(Self {
            db,
            sql_db,
            pkarr_client: pkarr_builder
                .clone()
                .build()
                .map_err(AppContextConversionError::Pkarr)?,
            file_service,
            pkarr_builder,
            config_toml: conf,
            keypair,
            data_dir: Arc::new(dir),
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

            // If we set custom bootstrap nodes, we don't want to use the default pkarr relay nodes.
            // Otherwise, we could end up with a DHT with testnet boostrap nodes and mainnet relays
            // which would give very weird results.
            builder.no_relays();
        }

        if let Some(relays) = &config_toml.pkdns.dht_relay_nodes {
            builder
                .relays(relays)
                .expect("parameters are already urls and therefore valid.");
        }
        if let Some(request_timeout) = &config_toml.pkdns.dht_request_timeout_ms {
            let duration = Duration::from_millis(request_timeout.get());
            builder.request_timeout(duration);
        }
        builder
    }

    /// Connect to the SQL database.
    /// If we are in a test environment and it's a test db connection string,
    /// we use an empheral test db.
    /// Otherwise, we use the normal db connection.
    async fn connect_to_sql_db(config_toml: &ConfigToml) -> Result<SqlDb, AppContextConversionError> {
        #[cfg(any(test, feature = "testing"))]
        {
        // If we are in a test environment and it's a test db connection string,
        // we use an empheral test db.
            if config_toml.general.db_url.is_test_db() {
                return Ok(SqlDb::test().await);
            } else {
                return Ok(SqlDb::new(&config_toml.general.db_url).await.map_err(AppContextConversionError::SqlDb)?);
            }
        }
        // If we are not in a test environment, we use the normal db connection.
        Ok(SqlDb::new(&config_toml.general.db_url).await.map_err(AppContextConversionError::SqlDb)?)
    }
}
