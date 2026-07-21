//!
//! The application context shared between all components.
//! Think of it as a simple Dependency Injection container.
//!
//! Create with a `DataDir` instance: `AppContext::try_from(data_dir)`
//!

use crate::services::user_service::UserService;
#[cfg(any(test, feature = "testing"))]
use crate::MockDataDir;
use crate::{
    observability::{Metrics, MetricsInitError},
    persistence::{
        files::{events::EventsService, FileIoError, FileService},
        sql::{ConnectionString, Migrator, PgEventListener, SqlDb},
    },
    ConfigToml, DataDir,
};
use pubky_common::crypto::Keypair;
use std::sync::Arc;
use std::time::Duration;

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
    /// Failed to open SQL DB.
    #[error("Failed to open SQL DB: {0}")]
    SqlDb(sqlx::Error),
    /// Failed to run migrations.
    #[error("Failed to run migrations: {0}")]
    Migrations(anyhow::Error),
    /// Failed to build storage operator.
    #[error("Failed to build storage operator: {0}")]
    Storage(FileIoError),
    /// Failed to build pkarr client.
    #[error("Failed to build pkarr client: {0}")]
    Pkarr(pkarr::errors::BuildError),
    /// Failed to start the Postgres event listener.
    #[error("Failed to start Postgres event listener: {0}")]
    PgEventListener(sqlx::Error),
    /// Failed to initialize metrics.
    #[error("Failed to initialize metrics: {0}")]
    Metrics(MetricsInitError),
    /// No database URL configured.
    #[error("No database_url configured. Set [general].database_url in config.toml.")]
    NoDatabaseUrl,
}

/// The application context shared between all components.
/// Think of it as a simple Dependency Injection container.
///
/// Create with a `DataDir` instance: `AppContext::try_from(data_dir)`
///
#[derive(Clone)]
pub struct AppContext {
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
    /// Events service for managing event creation and broadcasting.
    pub(crate) events_service: EventsService,
    /// Metrics for all endpoints.
    pub(crate) metrics: Metrics,
    /// Background listener for Postgres event notifications.
    /// Enables cross-instance event propagation for /events-stream's SSE functionality.
    /// Kept alive for the background task, not for direct access.
    _pg_event_listener: Arc<PgEventListener>,
    /// User service for quota resolution and user creation with defaults.
    pub(crate) user_service: UserService,
}

impl AppContext {
    /// Create a new AppContext for testing.
    #[cfg(any(test, feature = "testing"))]
    pub async fn test() -> Self {
        let data_dir = MockDataDir::test();
        Self::read_from(data_dir)
            .await
            .expect("failed to build AppContext from DataDirMock")
    }

    /// Create a new AppContext from a data directory.
    pub async fn read_from<D: DataDir + 'static>(
        dir: D,
    ) -> Result<Self, AppContextConversionError> {
        dir.ensure_data_dir_exists_and_is_writable()
            .map_err(AppContextConversionError::DataDir)?;
        let conf = dir
            .read_or_create_config_file()
            .map_err(AppContextConversionError::Config)?;
        let keypair = dir
            .read_or_create_keypair()
            .map_err(AppContextConversionError::Keypair)?;

        let sql_db = Self::connect_to_sql_db(&conf).await?;
        Migrator::new(&sql_db)
            .run()
            .await
            .map_err(AppContextConversionError::Migrations)?;

        let events_service = EventsService::new(1000);

        let pg_event_listener = PgEventListener::start(sql_db.pool(), events_service.clone())
            .await
            .map_err(AppContextConversionError::PgEventListener)?;

        let user_service = UserService::new(sql_db.clone());

        let file_service = FileService::new_from_config(
            &conf,
            dir.path(),
            sql_db.clone(),
            events_service.clone(),
            user_service.clone(),
        )
        .map_err(AppContextConversionError::Storage)?;
        let pkarr_builder = Self::build_pkarr_builder_from_config(&conf);

        Ok(Self {
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
            events_service,
            metrics: Metrics::new().map_err(AppContextConversionError::Metrics)?,
            _pg_event_listener: Arc::new(pg_event_listener),
            user_service,
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
    ///
    /// In test builds (`cfg(test)` or `feature = "testing"`), if no
    /// `database_url` is configured, resolves it via
    /// `TEST_PUBKY_CONNECTION_STRING` env var or the built-in default.
    async fn connect_to_sql_db(
        config_toml: &ConfigToml,
    ) -> Result<SqlDb, AppContextConversionError> {
        let con_string = Self::resolve_database_url(config_toml)?;
        SqlDb::connect(&con_string)
            .await
            .map_err(AppContextConversionError::SqlDb)
    }

    /// Resolve the database URL from config, falling back to test defaults
    /// when compiled with test support and no URL is configured.
    fn resolve_database_url(
        config_toml: &ConfigToml,
    ) -> Result<ConnectionString, AppContextConversionError> {
        match &config_toml.general.database_url {
            Some(url) => Ok(url.clone()),
            None => {
                #[cfg(any(test, feature = "testing"))]
                {
                    Ok(SqlDb::derive_connection_string(None))
                }
                #[cfg(not(any(test, feature = "testing")))]
                {
                    Err(AppContextConversionError::NoDatabaseUrl)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_database_url_returns_explicit_url() {
        let mut config = ConfigToml::default_test_config();
        let url = ConnectionString::new("postgres://localhost:9999/mydb").unwrap();
        config.general.database_url = Some(url.clone());

        let result = AppContext::resolve_database_url(&config).unwrap();
        assert_eq!(result, url);
    }

    #[test]
    fn resolve_database_url_falls_back_when_none() {
        let config = ConfigToml::default_test_config();
        assert!(config.general.database_url.is_none());

        let result = AppContext::resolve_database_url(&config).unwrap();
        // In test builds, None falls through to derive_connection_string(None),
        // which returns DEFAULT_TEST_CONNECTION_STRING (with ?pubky-test=true).
        assert!(result.is_test_db());
    }
}
