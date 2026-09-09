//!
//! The application context shared between all components.
//! Think of it as a simple Dependency Injection container.
//!
//! Build via [`AppContext::new`] with independently resolved:
//! - **Data path** — persistent directory or temp dir
//! - **Database mode** — [`DatabaseMode::Direct`] or `DatabaseMode::EphemeralTest`
//! - **pkarr builder** — a pre-configured [`pkarr::ClientBuilder`]
//!
//! Convenience constructors:
//! - [`AppContext::from_persistent_dir`] — production (persistent dir, public DHT, direct DB)
//! - `AppContext::new_ephemeral` — tests (temp dir, isolated DHT, test DB)
//!

use crate::services::user_service::UserService;
use crate::{
    client_server::auth::RevocationListener,
    observability::{Metrics, MetricsInitError},
    persistence::{
        files::{events::EventsService, FileIoError, FileService},
        sql::{DatabaseMode, Migrator, PgEventListener, SqlDb},
    },
    ConfigToml,
};
use pubky_common::crypto::Keypair;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Errors that can occur when building an `AppContext`.
#[derive(Debug, thiserror::Error)]
pub enum AppContextBuildError {
    /// Failed to bootstrap the data directory (ensure writable, read config/keypair).
    #[error("Failed to bootstrap data directory: {0}")]
    Bootstrap(anyhow::Error),
    /// Failed to open SQL DB.
    #[error("Failed to open SQL DB: {0}")]
    SqlDb(sqlx::Error),
    /// Failed to resolve the database mode (e.g. missing URL or invalid TEST_PUBKY_CONNECTION_STRING).
    #[error("Failed to resolve database mode: {0}")]
    DatabaseResolution(anyhow::Error),
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
    /// Failed to start the auth revocation listener.
    #[error("Failed to start the auth revocation listener: {0}")]
    RevocationListener(sqlx::Error),
    /// Failed to initialize metrics.
    #[error("Failed to initialize metrics: {0}")]
    Metrics(MetricsInitError),
}

/// The application context shared between all components.
/// Think of it as a simple Dependency Injection container.
///
/// Implements `Clone` but prefer wrapping in `Arc<AppContext>` for
/// hot paths like axum state (avoids deep-copying config strings).
#[derive(Clone)]
pub struct AppContext {
    /// The SQL database connection.
    pub(crate) sql_db: SqlDb,
    /// The storage operator to store files.
    pub(crate) file_service: FileService,
    pub(crate) config_toml: ConfigToml,
    /// Path to the data directory (used by file storage).
    pub(crate) data_path: PathBuf,
    /// Keeps an ephemeral data directory alive for as long as any clone of this
    /// context exists, so `data_path` can never outlive the directory it names.
    /// `None` for persistent data directories, which the caller owns.
    #[cfg(any(test, feature = "testing"))]
    _temp_dir: Option<Arc<tempfile::TempDir>>,
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
    /// Auth revocations are forwarded to private SSE streams on this instance.
    /// Its Postgres listener stops once the last clone is dropped.
    pub(crate) revocation_listener: RevocationListener,
    /// User service for quota resolution and user creation with defaults.
    pub(crate) user_service: UserService,
}

impl AppContext {
    /// Production shorthand: persistent data dir, public DHT, direct DB.
    ///
    /// Reads config and keypair from disk via [`crate::PersistentDataDir::bootstrap`].
    pub async fn from_persistent_dir(
        dir: crate::PersistentDataDir,
    ) -> Result<Self, AppContextBuildError> {
        let (path, config, keypair) = dir.bootstrap().map_err(AppContextBuildError::Bootstrap)?;
        let db_mode = DatabaseMode::require_direct(config.general.database_url.clone())
            .map_err(AppContextBuildError::DatabaseResolution)?;
        Self::warn_on_mixed_dht_network(&config);
        Self::new(
            path,
            config,
            keypair,
            db_mode,
            pkarr::ClientBuilder::default(),
        )
        .await
    }

    /// Warn when a custom DHT is paired with the public pkarr relays.
    ///
    /// Setting `dht_bootstrap_nodes` no longer clears the relays, so records published
    /// to a private DHT also reach `pkarr.pubky.app` / `.org`, and resolution mixes both
    /// networks. Earlier versions silently called `no_relays()` here; clearing the relays
    /// is now an explicit `dht_relay_nodes = []`, so warn rather than let an upgrade
    /// quietly change where records end up.
    fn warn_on_mixed_dht_network(config: &ConfigToml) {
        if Self::mixes_private_dht_with_public_relays(config) {
            tracing::warn!(
                "[pkdns] sets custom `dht_bootstrap_nodes` while `dht_relay_nodes` is still \
                 the public default ({}). This homeserver will publish its pkarr record to \
                 the public relays as well as to your DHT, and resolve from both networks. \
                 Set `dht_relay_nodes = []` to stay off the public relays, or list the relays \
                 you want. Earlier versions disabled the relays here automatically.",
                pkarr::DEFAULT_RELAYS.join(", "),
            );
        }
    }

    /// Whether this config joins a custom DHT while still pointing at the public relays.
    ///
    /// Note that a config read from a file always carries `dht_relay_nodes` — the embedded
    /// default fills it in with [`pkarr::DEFAULT_RELAYS`] — so "the user did not choose
    /// relays" means *unset or still equal to those defaults*, not just `None`.
    /// See [`warn_on_mixed_dht_network`](Self::warn_on_mixed_dht_network).
    fn mixes_private_dht_with_public_relays(config: &ConfigToml) -> bool {
        let has_custom_bootstrap = config
            .pkdns
            .dht_bootstrap_nodes
            .as_ref()
            .is_some_and(|nodes| !nodes.is_empty());
        if !has_custom_bootstrap {
            return false;
        }
        match &config.pkdns.dht_relay_nodes {
            // Unset: pkarr's own public defaults apply.
            None => true,
            // Still (or partly) the public defaults: not a deliberate choice.
            Some(relays) => relays.iter().any(Self::is_default_public_relay),
        }
    }

    /// Whether `relay` is one of pkarr's public default relays, ignoring a trailing slash.
    fn is_default_public_relay(relay: &url::Url) -> bool {
        let relay = relay.as_str().trim_end_matches('/');
        pkarr::DEFAULT_RELAYS
            .iter()
            .any(|default| relay == default.trim_end_matches('/'))
    }

    /// Quick test context with default config and a deterministic keypair.
    #[cfg(any(test, feature = "testing"))]
    pub async fn test() -> Arc<Self> {
        Arc::new(
            Self::new_ephemeral(
                ConfigToml::default_test_config(),
                Keypair::from_secret(&[0; 32]),
                None,
            )
            .await
            .expect("failed to build test AppContext"),
        )
    }

    /// Quick test context with a custom config modifier and a random keypair.
    #[cfg(any(test, feature = "testing"))]
    pub async fn test_with_config(f: impl FnOnce(&mut ConfigToml)) -> Arc<Self> {
        let mut config = ConfigToml::default_test_config();
        f(&mut config);
        Arc::new(
            Self::new_ephemeral(config, Keypair::random(), None)
                .await
                .expect("failed to build test AppContext"),
        )
    }

    /// Test shorthand: temp data dir, isolated DHT, ephemeral test DB.
    ///
    /// The context owns the temporary directory, so it is removed once the last
    /// clone of the context is dropped — there is no lifetime to manage.
    ///
    /// `database_override` is the top tier of [`DatabaseMode::resolve_test`]: pass a
    /// connection string here to pin the database ahead of `TEST_PUBKY_CONNECTION_STRING`
    /// and `config.general.database_url`, or `None` to let those decide.
    #[cfg(any(test, feature = "testing"))]
    pub async fn new_ephemeral(
        config: ConfigToml,
        keypair: Keypair,
        database_override: Option<crate::persistence::sql::ConnectionString>,
    ) -> Result<Self, AppContextBuildError> {
        let pkarr_builder = Self::isolated_pkarr_builder(&config);
        Self::new_ephemeral_with_pkarr(config, keypair, database_override, pkarr_builder).await
    }

    /// Like [`new_ephemeral`](Self::new_ephemeral) but with a caller-supplied
    /// pkarr builder — e.g. one pointed at a `mainline::Testnet`.
    ///
    /// As in [`new`](Self::new), the config's `[pkdns]` settings are applied on top of
    /// the given builder, so a config carrying `dht_bootstrap_nodes` overrides the
    /// builder's network.
    #[cfg(any(test, feature = "testing"))]
    pub async fn new_ephemeral_with_pkarr(
        config: ConfigToml,
        keypair: Keypair,
        database_override: Option<crate::persistence::sql::ConnectionString>,
        pkarr_builder: pkarr::ClientBuilder,
    ) -> Result<Self, AppContextBuildError> {
        let temp_dir =
            tempfile::TempDir::new().map_err(|e| AppContextBuildError::Bootstrap(e.into()))?;
        let data_path = temp_dir.path().to_path_buf();
        let db_mode =
            DatabaseMode::resolve_test(database_override, config.general.database_url.clone())
                .map_err(AppContextBuildError::DatabaseResolution)?;

        let mut ctx = Self::new(data_path, config, keypair, db_mode, pkarr_builder).await?;
        ctx._temp_dir = Some(Arc::new(temp_dir));
        Ok(ctx)
    }

    /// Create an `AppContext` from independently resolved components.
    ///
    /// Each parameter represents a separate concern:
    /// - `data_path` — where file storage lives (persistent dir or temp dir)
    /// - `config` — homeserver configuration
    /// - `keypair` — server identity
    /// - `db_mode` — database lifecycle ([`DatabaseMode::Direct`] or `DatabaseMode::EphemeralTest`)
    /// - `pkarr_builder` — the base [`pkarr::ClientBuilder`], supplying the network to
    ///   join (public by default, or a `mainline::Testnet`) and any transport settings
    ///
    /// The `[pkdns]` DHT settings from `config` are applied on top of `pkarr_builder`,
    /// so a caller can never silently lose them by passing a builder of their own.
    /// Config wins for bootstrap nodes, relays and request timeout; the builder supplies
    /// everything else.
    ///
    /// `data_path` must already exist and be writable — this constructor does not create
    /// it. The convenience constructors handle that for you
    /// ([`from_persistent_dir`](Self::from_persistent_dir) via
    /// [`PersistentDataDir::bootstrap`](crate::PersistentDataDir::bootstrap), `new_ephemeral`
    /// via a temp dir); a caller assembling the parts itself owns that step.
    ///
    /// See [`from_persistent_dir`](Self::from_persistent_dir) and
    /// `new_ephemeral` for common combinations.
    pub async fn new(
        data_path: PathBuf,
        config: ConfigToml,
        keypair: Keypair,
        db_mode: DatabaseMode,
        mut pkarr_builder: pkarr::ClientBuilder,
    ) -> Result<Self, AppContextBuildError> {
        Self::apply_config_to_pkarr(&mut pkarr_builder, &config);
        let sql_db = SqlDb::connect(db_mode)
            .await
            .map_err(AppContextBuildError::SqlDb)?;
        Migrator::new(&sql_db)
            .run()
            .await
            .map_err(AppContextBuildError::Migrations)?;

        let events_service = EventsService::new(sql_db.clone(), 1000);

        let pg_event_listener = PgEventListener::start(sql_db.pool(), events_service.clone())
            .await
            .map_err(AppContextBuildError::PgEventListener)?;
        let revocation_listener = RevocationListener::start(sql_db.pool())
            .await
            .map_err(AppContextBuildError::RevocationListener)?;

        let user_service = UserService::new(sql_db.clone());

        let file_service = FileService::new_from_config(
            &config,
            &data_path,
            sql_db.clone(),
            events_service.clone(),
            user_service.clone(),
        )
        .map_err(AppContextBuildError::Storage)?;

        Ok(Self {
            sql_db,
            pkarr_client: pkarr_builder
                .clone()
                .build()
                .map_err(AppContextBuildError::Pkarr)?,
            file_service,
            pkarr_builder,
            config_toml: config,
            keypair,
            data_path,
            #[cfg(any(test, feature = "testing"))]
            _temp_dir: None,
            events_service,
            metrics: Metrics::new().map_err(AppContextBuildError::Metrics)?,
            _pg_event_listener: Arc::new(pg_event_listener),
            revocation_listener,
            user_service,
        })
    }

    /// Create a pkarr builder isolated from the public network, with config applied.
    ///
    /// Starts from a blank network (no default bootstrap nodes or relays, testnet
    /// report policy). Config values are applied on top — used by testnets which
    /// inject their own bootstrap/relay nodes via config.
    #[cfg(any(test, feature = "testing"))]
    pub fn isolated_pkarr_builder(config: &ConfigToml) -> pkarr::ClientBuilder {
        let mut builder = pkarr::ClientBuilder::default();
        builder
            .no_default_network()
            // Sentinel bootstrap node so the builder stays valid even when
            // no config-level bootstrap nodes are provided. Explicit testnet
            // bootstrap nodes (from config) replace this via apply_config_to_pkarr.
            // Port 9 is the RFC 863 "discard" protocol — guaranteed unreachable as a DHT node.
            .bootstrap(&["127.0.0.1:9"])
            .dht_report_policy(pkarr::dht::ReportPolicy::testnet());
        Self::apply_config_to_pkarr(&mut builder, config);
        builder
    }

    /// Apply DHT configuration (bootstrap nodes, relays, timeouts) from config
    /// to a pkarr client builder.
    ///
    /// An empty list means "none" for both `dht_bootstrap_nodes` and `dht_relay_nodes`:
    /// no bootstrap nodes starts an isolated DHT, no relays disables relays entirely.
    fn apply_config_to_pkarr(builder: &mut pkarr::ClientBuilder, config: &ConfigToml) {
        if let Some(bootstrap_nodes) = &config.pkdns.dht_bootstrap_nodes {
            if bootstrap_nodes.is_empty() {
                tracing::warn!(
                    "`dht_bootstrap_nodes = []` under [pkdns] starts an isolated DHT with no \
                     peers: this homeserver will not resolve or publish any records over the \
                     DHT. Remove the key to use the default bootstrap nodes."
                );
            }
            let nodes = bootstrap_nodes
                .iter()
                .map(|node| node.to_string())
                .collect::<Vec<String>>();
            builder.bootstrap(&nodes);
        }

        if let Some(relays) = &config.pkdns.dht_relay_nodes {
            if relays.is_empty() {
                builder.no_relays();
            } else {
                builder
                    .relays(relays)
                    .expect("parameters are already URLs and therefore valid.");
            }
        }
        if let Some(request_timeout) = &config.pkdns.dht_request_timeout_ms {
            let duration = Duration::from_millis(request_timeout.get());
            builder.request_timeout(duration);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The context owns its ephemeral data directory: it survives as long as any clone
    /// does, and goes away with the last one. This is why `AppContext` holds the
    /// `TempDir` rather than handing it back for the caller to keep alive.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn ephemeral_data_dir_outlives_every_clone_of_the_context() {
        let context =
            AppContext::new_ephemeral(ConfigToml::default_test_config(), Keypair::random(), None)
                .await
                .expect("failed to build ephemeral AppContext");
        let data_path = context.data_path.clone();
        assert!(
            data_path.is_dir(),
            "the temp data dir should exist up front"
        );

        let clone = context.clone();
        drop(context);
        assert!(
            data_path.is_dir(),
            "a surviving clone must keep the data dir alive"
        );

        drop(clone);
        assert!(
            !data_path.exists(),
            "dropping the last clone must remove the data dir"
        );
    }

    fn config_with_bootstrap(nodes: &[&str]) -> ConfigToml {
        use crate::DomainPort;
        use std::str::FromStr;

        let mut config = ConfigToml::default_test_config();
        config.pkdns.dht_bootstrap_nodes = Some(
            nodes
                .iter()
                .map(|n| DomainPort::from_str(n).unwrap())
                .collect(),
        );
        config
    }

    /// Custom bootstrap nodes with no explicit relays leaves the public relays active.
    /// That is a deliberate change from the old implicit `no_relays()`, so it has to be
    /// warned about — see `warn_on_mixed_dht_network`.
    #[test]
    fn mixed_network_is_detected_when_relays_are_unset() {
        let config = config_with_bootstrap(&["127.0.0.1:6881"]);
        assert_eq!(config.pkdns.dht_relay_nodes, None);
        assert!(AppContext::mixes_private_dht_with_public_relays(&config));
    }

    /// The case that actually reaches production: a config read from disk always carries
    /// `dht_relay_nodes`, because the embedded default fills it in with the public relays.
    /// Checking for `None` alone would never fire.
    #[test]
    fn mixed_network_is_detected_for_a_config_read_from_a_file() {
        let config = ConfigToml::from_str_with_defaults(
            "[pkdns]\ndht_bootstrap_nodes = [\"my-dht.internal:6881\"]\n",
        )
        .unwrap();

        assert!(
            config
                .pkdns
                .dht_relay_nodes
                .as_ref()
                .is_some_and(|r| !r.is_empty()),
            "the embedded default should have filled in the public relays"
        );
        assert!(
            AppContext::mixes_private_dht_with_public_relays(&config),
            "a private DHT left on the default public relays must be flagged"
        );
    }

    #[test]
    fn mixed_network_is_not_flagged_once_relays_are_explicit() {
        let mut config = config_with_bootstrap(&["127.0.0.1:6881"]);

        config.pkdns.dht_relay_nodes = Some(vec![]);
        assert!(
            !AppContext::mixes_private_dht_with_public_relays(&config),
            "`dht_relay_nodes = []` is the documented opt-out"
        );

        config.pkdns.dht_relay_nodes =
            Some(vec![url::Url::parse("https://relay.example").unwrap()]);
        assert!(
            !AppContext::mixes_private_dht_with_public_relays(&config),
            "an explicit private relay list is a deliberate choice"
        );
    }

    #[test]
    fn default_public_relays_are_recognised_with_or_without_trailing_slash() {
        for relay in pkarr::DEFAULT_RELAYS {
            assert!(AppContext::is_default_public_relay(
                &url::Url::parse(relay).unwrap()
            ));
            assert!(
                AppContext::is_default_public_relay(
                    &url::Url::parse(&format!("{relay}/")).unwrap()
                ),
                "url::Url normalises {relay} to a trailing slash, which must still match"
            );
        }
        assert!(!AppContext::is_default_public_relay(
            &url::Url::parse("https://relay.example").unwrap()
        ));
    }

    #[test]
    fn mixed_network_is_not_flagged_on_the_default_public_network() {
        let config = ConfigToml::default_test_config();
        assert_eq!(config.pkdns.dht_bootstrap_nodes, None);
        assert!(
            !AppContext::mixes_private_dht_with_public_relays(&config),
            "default bootstrap nodes and default relays are the same network"
        );

        assert!(
            !AppContext::mixes_private_dht_with_public_relays(&config_with_bootstrap(&[])),
            "an empty bootstrap list is not a custom DHT"
        );
    }

    /// An empty bootstrap list means "no bootstrap nodes", mirroring the relay handling.
    #[test]
    fn empty_bootstrap_list_clears_the_default_nodes() {
        let mut builder = pkarr::ClientBuilder::default();
        AppContext::apply_config_to_pkarr(&mut builder, &config_with_bootstrap(&[]));
        let debug = format!("{builder:?}");

        assert!(
            debug.contains("bootstrap: Some([])"),
            "an empty list should reach pkarr as an empty bootstrap set: {debug}"
        );
        builder
            .build()
            .expect("a peerless DHT client should still build");
    }

    /// Public (default) builder keeps default relays and applies config values.
    #[test]
    fn public_builder_keeps_defaults_and_applies_config() {
        use crate::DomainPort;
        use std::str::FromStr;

        let mut config = ConfigToml::default_test_config();
        config.pkdns.dht_bootstrap_nodes =
            Some(vec![DomainPort::from_str("127.0.0.1:6881").unwrap()]);

        let mut builder = pkarr::ClientBuilder::default();
        AppContext::apply_config_to_pkarr(&mut builder, &config);
        let debug = format!("{builder:?}");

        assert!(
            debug.contains("127.0.0.1:6881"),
            "bootstrap node from config should be present: {debug}"
        );
        for relay in pkarr::DEFAULT_RELAYS {
            assert!(
                debug.contains(relay),
                "default relay {relay} should still be present: {debug}"
            );
        }
    }

    /// Isolated builder excludes public DHT nodes.
    #[test]
    fn isolated_builder_excludes_public_dht() {
        let builder = AppContext::isolated_pkarr_builder(&ConfigToml::default_test_config());
        let debug = format!("{builder:?}");

        for relay in pkarr::DEFAULT_RELAYS {
            assert!(
                !debug.contains(relay),
                "default relay {relay} should not appear after no_default_network: {debug}"
            );
        }
        builder.build().expect("isolated pkarr client should build");
    }
}
