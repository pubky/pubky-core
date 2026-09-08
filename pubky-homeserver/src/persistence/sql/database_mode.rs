use super::connection_string::ConnectionString;

/// How the homeserver should connect to its database.
///
/// This enum makes the distinction between "connect to an existing database"
/// and "create a fresh ephemeral database for this test" explicit.
#[derive(Debug, Clone)]
pub enum DatabaseMode {
    /// Connect directly to the database identified by the URL.
    /// Used in production and persistent testnets.
    Direct(ConnectionString),

    /// Create an ephemeral `pubky_test_{uuid}` database on the server
    /// identified by the URL, then connect to it.
    ///
    /// Dropping the [`SqlDb`](super::SqlDb) **registers** the database for
    /// cleanup but does not delete it immediately. Actual deletion requires
    /// the `#[pubky_testnet::test]` macro or an explicit call to
    /// [`drop_test_databases()`](pubky_test_utils::drop_test_databases).
    /// Without either, the database will be leaked.
    ///
    /// Only available in test / testing builds.
    #[cfg(any(test, feature = "testing"))]
    EphemeralTest(ConnectionString),
}

impl DatabaseMode {
    /// Require an explicit database URL, returning `Direct` mode.
    ///
    /// Returns an error when the URL is `None` — use this for production
    /// and persistent-testnet paths where a database URL must be configured.
    pub fn require_direct(url: Option<ConnectionString>) -> anyhow::Result<Self> {
        url.map(Self::Direct).ok_or_else(|| {
            anyhow::anyhow!(
                "No database_url configured. Set [general].database_url in config.toml."
            )
        })
    }

    /// Returns the underlying connection string, regardless of mode.
    #[cfg(test)]
    pub fn connection_string(&self) -> &ConnectionString {
        match self {
            Self::Direct(url) => url,
            #[cfg(any(test, feature = "testing"))]
            Self::EphemeralTest(url) => url,
        }
    }
}

#[cfg(any(test, feature = "testing"))]
const DEFAULT_TEST_SERVER: &str = "postgres://localhost:5432/postgres";

#[cfg(any(test, feature = "testing"))]
impl DatabaseMode {
    /// Pick the ephemeral test database, applying the shared precedence rule in
    /// [`ConnectionString::resolve_for_test`] and falling back to
    /// [`DEFAULT_TEST_SERVER`] when nothing is configured:
    ///
    /// 1. `override_url` (e.g. docker postgres or `EphemeralTestnetBuilder::postgres`)
    /// 2. `TEST_PUBKY_CONNECTION_STRING`
    /// 3. `from_config` — `[general].database_url`
    /// 4. [`DEFAULT_TEST_SERVER`]
    ///
    /// The result is always [`EphemeralTest`](Self::EphemeralTest): tests get a fresh
    /// database on the chosen server, never the server's own database.
    pub fn resolve_test(
        override_url: Option<ConnectionString>,
        from_config: Option<ConnectionString>,
    ) -> anyhow::Result<Self> {
        let url =
            ConnectionString::resolve_for_test(override_url, from_config)?.unwrap_or_else(|| {
                ConnectionString::new(DEFAULT_TEST_SERVER)
                    .expect("Default test connection string is valid")
            });
        Ok(Self::EphemeralTest(url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_direct_returns_direct_when_url_set() {
        let url = ConnectionString::new("postgres://localhost:5432/mydb").unwrap();
        let mode = DatabaseMode::require_direct(Some(url.clone())).unwrap();
        assert!(
            matches!(mode, DatabaseMode::Direct(_)),
            "require_direct should return Direct"
        );
        assert_eq!(mode.connection_string(), &url);
    }

    #[test]
    fn require_direct_errors_when_no_url() {
        let result = DatabaseMode::require_direct(None);
        assert!(
            result.is_err(),
            "require_direct should error when url is None"
        );
    }

    #[test]
    fn resolve_test_override_wins_and_is_ephemeral() {
        // An override short-circuits the env lookup, so this is independent of
        // whatever TEST_PUBKY_CONNECTION_STRING happens to be set to.
        let override_url = ConnectionString::new("postgres://custom:5432/mydb").unwrap();
        let mode = DatabaseMode::resolve_test(
            Some(override_url.clone()),
            Some(ConnectionString::new("postgres://config:5432/db").unwrap()),
        )
        .unwrap();
        assert_eq!(mode.connection_string(), &override_url);
        assert!(
            matches!(mode, DatabaseMode::EphemeralTest(_)),
            "tests always get an ephemeral database, never Direct"
        );
    }

    #[test]
    fn resolve_test_keeps_old_style_pubky_test_param_url() {
        let url =
            ConnectionString::new("postgres://user:pass@localhost:5432/postgres?pubky-test=true")
                .unwrap();
        let mode = DatabaseMode::resolve_test(Some(url), None).unwrap();
        assert!(matches!(mode, DatabaseMode::EphemeralTest(_)));
    }

    #[test]
    fn resolve_test_uses_the_config_then_the_default_server() {
        // With no override the env var is consulted, so only assert the lower tiers
        // when the ambient environment leaves it unset.
        if std::env::var(super::super::connection_string::TEST_CONNECTION_STRING_ENV).is_ok() {
            return;
        }

        let from_config = ConnectionString::new("postgres://config:5432/db").unwrap();
        let mode = DatabaseMode::resolve_test(None, Some(from_config.clone())).unwrap();
        assert_eq!(mode.connection_string(), &from_config);

        let mode = DatabaseMode::resolve_test(None, None).unwrap();
        assert_eq!(mode.connection_string().as_str(), DEFAULT_TEST_SERVER);
        assert!(matches!(mode, DatabaseMode::EphemeralTest(_)));
    }
}
