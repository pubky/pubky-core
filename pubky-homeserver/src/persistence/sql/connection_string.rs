use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// Environment variable that overrides the database URL in test / testing builds.
#[cfg(any(test, feature = "testing"))]
pub const TEST_CONNECTION_STRING_ENV: &str = "TEST_PUBKY_CONNECTION_STRING";

/// A connection string for a  postgres database.
/// See <https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING-URIS>
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionString(url::Url);

impl ConnectionString {
    /// Create a new connection string from a string.
    /// This function validates that the connection string is a postgres connection string.
    pub fn new(con_string: &str) -> anyhow::Result<Self> {
        Self::validated(url::Url::parse(con_string)?)
    }

    /// Shared validation: ensures the URL uses a postgres scheme.
    fn validated(url: url::Url) -> anyhow::Result<Self> {
        let cs = Self(url);
        if !cs.is_postgres() {
            anyhow::bail!("Only postgres database urls are supported");
        }
        Ok(cs)
    }

    /// Get the connection string as a str.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// **The** precedence rule for choosing a database in test and testnet builds.
    /// Every test/testnet code path resolves through this one function.
    ///
    /// Highest first:
    /// 1. `override_url` — a connection string chosen in code, e.g.
    ///    [`EphemeralTestnetBuilder::postgres`] or a docker-postgres container.
    /// 2. [`TEST_CONNECTION_STRING_ENV`] — the ambient environment.
    /// 3. `from_config` — `[general].database_url`.
    ///
    /// Returns `None` when none of the three is set; the caller decides what that means
    /// ([`DatabaseMode::resolve_test`] falls back to a default server, the persistent
    /// testnet treats it as a configuration error).
    ///
    /// The env var deliberately sits *above* the config, following the usual
    /// argument → environment → config-file → default convention, and because a
    /// `database_url` in a config file is often just the embedded default rather than
    /// something anyone chose. A caller that must pin a specific database regardless of
    /// the environment passes it as `override_url`.
    ///
    /// [`DatabaseMode::resolve_test`]: super::DatabaseMode::resolve_test
    /// [`EphemeralTestnetBuilder::postgres`]: https://docs.rs/pubky-testnet
    #[cfg(any(test, feature = "testing"))]
    pub fn resolve_for_test(
        override_url: Option<Self>,
        from_config: Option<Self>,
    ) -> anyhow::Result<Option<Self>> {
        // Skip the env lookup entirely when an override is present, so an unparseable
        // env var cannot fail a run that was never going to consult it.
        let from_env = match override_url {
            Some(_) => None,
            None => Self::from_test_env()?,
        };
        Ok(Self::pick(override_url, from_env, from_config))
    }

    /// Pure precedence logic, separated from env access so it can be tested
    /// without mutating the process environment.
    #[cfg(any(test, feature = "testing"))]
    fn pick(
        override_url: Option<Self>,
        from_env: Option<Self>,
        from_config: Option<Self>,
    ) -> Option<Self> {
        override_url.or(from_env).or(from_config)
    }

    /// Read a connection string from the `TEST_PUBKY_CONNECTION_STRING` environment variable.
    ///
    /// Returns `Ok(None)` if the variable is unset.
    /// Returns `Err` if the variable is set but contains an invalid URL.
    #[cfg(any(test, feature = "testing"))]
    pub fn from_test_env() -> anyhow::Result<Option<Self>> {
        Self::parse_env_value(std::env::var(TEST_CONNECTION_STRING_ENV))
    }

    /// Pure parsing logic, separated from env access so it can be tested without
    /// mutating the process environment (which would race with the many tests
    /// that read `TEST_PUBKY_CONNECTION_STRING` concurrently).
    #[cfg(any(test, feature = "testing"))]
    fn parse_env_value(raw: Result<String, std::env::VarError>) -> anyhow::Result<Option<Self>> {
        let raw = match raw {
            Ok(val) => val,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(e) => anyhow::bail!("Invalid {TEST_CONNECTION_STRING_ENV}: {e}"),
        };
        let cs = Self::new(&raw)
            .map_err(|e| anyhow::anyhow!("Invalid {TEST_CONNECTION_STRING_ENV} ({raw:?}): {e}"))?;
        Ok(Some(cs))
    }

    fn is_postgres(&self) -> bool {
        self.0.scheme() == "postgres" || self.0.scheme() == "postgresql"
    }

    /// Get the database name
    /// For postgres, this is the database name directly
    pub fn database_name(&self) -> &str {
        self.0.path().trim_start_matches("/")
    }

    /// Set the database name, clearing any `dbname` query parameter that would
    /// otherwise override the path. See
    /// <https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING-URIS>
    pub fn set_database_name(&mut self, db_name: &str) {
        self.0.set_path(db_name);
        self.remove_query_param("dbname");
    }

    /// Remove all occurrences of a query parameter by key.
    fn remove_query_param(&mut self, key: &str) {
        if self.0.query().is_none() {
            return;
        }
        let pairs: Vec<_> = self
            .0
            .query_pairs()
            .filter(|(k, _)| k != key)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if pairs.is_empty() {
            self.0.set_query(None);
        } else {
            self.0.query_pairs_mut().clear().extend_pairs(&pairs);
        }
    }
}

impl TryFrom<url::Url> for ConnectionString {
    type Error = anyhow::Error;

    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        Self::validated(url)
    }
}

impl FromStr for ConnectionString {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl Display for ConnectionString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for ConnectionString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ConnectionString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(url: &str) -> ConnectionString {
        ConnectionString::new(url).unwrap()
    }

    // --- Precedence: explicit -> env -> fallback ------------------------------

    #[test]
    fn pick_override_wins_over_env_and_config() {
        let override_url = cs("postgres://custom:5432/mydb");
        let picked = ConnectionString::pick(
            Some(override_url.clone()),
            Some(cs("postgres://envhost:5432/envdb")),
            Some(cs("postgres://confighost:5432/configdb")),
        );
        assert_eq!(picked, Some(override_url));
    }

    #[test]
    fn pick_env_wins_over_config() {
        let env = cs("postgres://envhost:5432/envdb");
        let picked = ConnectionString::pick(
            None,
            Some(env.clone()),
            Some(cs("postgres://confighost:5432/configdb")),
        );
        assert_eq!(
            picked,
            Some(env),
            "the env var has to beat the config, whose database_url is filled in by \
             the embedded default even when the user never set one; a caller that must \
             pin a database passes it as the override instead"
        );
    }

    #[test]
    fn pick_uses_the_config_when_nothing_outranks_it() {
        let from_config = cs("postgres://confighost:5432/configdb");
        let picked = ConnectionString::pick(None, None, Some(from_config.clone()));
        assert_eq!(picked, Some(from_config));
    }

    #[test]
    fn pick_returns_none_when_nothing_is_set() {
        assert_eq!(ConnectionString::pick(None, None, None), None);
    }

    #[test]
    fn resolve_for_test_skips_env_lookup_when_an_override_is_set() {
        // An override must short-circuit before `from_test_env`, so this holds
        // regardless of what the ambient environment contains.
        let override_url = cs("postgres://custom:5432/mydb");
        let resolved = ConnectionString::resolve_for_test(
            Some(override_url.clone()),
            Some(cs("postgres://config:5432/db")),
        )
        .unwrap();
        assert_eq!(resolved, Some(override_url));
    }

    #[test]
    fn resolve_for_test_falls_through_to_the_config_when_nothing_overrides_it() {
        // Only meaningful with the env var unset; skip otherwise rather than mutate it.
        if std::env::var(TEST_CONNECTION_STRING_ENV).is_ok() {
            return;
        }
        let from_config = cs("postgres://config:5432/db");
        assert_eq!(
            ConnectionString::resolve_for_test(None, Some(from_config.clone())).unwrap(),
            Some(from_config)
        );
        assert_eq!(
            ConnectionString::resolve_for_test(None, None).unwrap(),
            None,
            "nothing configured anywhere leaves the choice to the caller"
        );
    }

    // --- Env var parsing -----------------------------------------------------

    #[test]
    fn parse_env_value_unset_is_none() {
        let parsed =
            ConnectionString::parse_env_value(Err(std::env::VarError::NotPresent)).unwrap();
        assert_eq!(parsed, None);
    }

    #[test]
    fn parse_env_value_reads_a_valid_url() {
        let parsed =
            ConnectionString::parse_env_value(Ok("postgres://envhost:5432/envdb".to_string()))
                .unwrap();
        assert_eq!(parsed.unwrap().as_str(), "postgres://envhost:5432/envdb");
    }

    #[test]
    fn parse_env_value_rejects_an_invalid_url() {
        let err = ConnectionString::parse_env_value(Ok("not-a-valid-url".to_string()))
            .expect_err("an unparseable url should be an error, not a silent fallback");
        let msg = err.to_string();
        assert!(
            msg.contains(TEST_CONNECTION_STRING_ENV) && msg.contains("not-a-valid-url"),
            "error should name the env var and the offending value, got: {msg}"
        );
    }

    #[test]
    fn parse_env_value_rejects_a_non_postgres_url() {
        assert!(
            ConnectionString::parse_env_value(Ok("sqlite:///tmp/db.sqlite".to_string())).is_err(),
            "only postgres urls are supported"
        );
    }

    #[test]
    fn parse_env_value_rejects_an_empty_string() {
        assert!(
            ConnectionString::parse_env_value(Ok(String::new())).is_err(),
            "an empty env var should error rather than silently fall through"
        );
    }

    #[test]
    fn parse_env_value_rejects_non_unicode() {
        assert!(
            ConnectionString::parse_env_value(Err(std::env::VarError::NotUnicode(
                "\u{fffd}".into()
            )))
            .is_err()
        );
    }

    #[test]
    fn parse_env_value_accepts_old_style_url_with_pubky_test_param() {
        // Older setups carried the ephemeral-database decision in the URL itself.
        // Those URLs must still parse; the `pubky-test` param is simply ignored now.
        let parsed = ConnectionString::parse_env_value(Ok(
            "postgres://user:pass@localhost:5432/postgres?pubky-test=true".to_string(),
        ))
        .unwrap();
        assert!(parsed.is_some());
    }

    #[test]
    fn test_valid_postgres_url() {
        let _: ConnectionString = "postgres://localhost:5432/pubky_homeserver"
            .parse()
            .unwrap();
    }

    #[test]
    fn test_non_postgres_url_rejected() {
        let result: Result<ConnectionString, _> = "sqlite:///path/to/sqlite.db".parse();
        assert!(result.is_err(), "sqlite URLs should be rejected");
    }

    #[test]
    fn set_database_name_changes_path() {
        let mut cs = ConnectionString::new("postgres://user:pass@localhost:5432/original").unwrap();
        cs.set_database_name("new_db");
        assert_eq!(cs.database_name(), "new_db");
    }

    #[test]
    fn set_database_name_strips_dbname_query_param() {
        let mut cs =
            ConnectionString::new("postgres://user:pass@localhost:5432/postgres?dbname=postgres")
                .unwrap();
        cs.set_database_name("pubky_test_abc123");
        assert_eq!(cs.database_name(), "pubky_test_abc123");
        assert!(
            !cs.as_str().contains("dbname="),
            "dbname query param should be removed, got: {}",
            cs.as_str()
        );
    }

    #[test]
    fn set_database_name_preserves_other_query_params() {
        let mut cs = ConnectionString::new(
            "postgres://user:pass@localhost:5432/postgres?dbname=postgres&sslmode=require",
        )
        .unwrap();
        cs.set_database_name("pubky_test_abc123");
        assert_eq!(cs.database_name(), "pubky_test_abc123");
        assert!(
            !cs.as_str().contains("dbname="),
            "dbname should be removed, got: {}",
            cs.as_str()
        );
        assert!(
            cs.as_str().contains("sslmode=require"),
            "other params should be preserved, got: {}",
            cs.as_str()
        );
    }
}
