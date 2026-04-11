use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// A connection string for a  postgres database.
/// See https://www.postgresql.org/docs/current/libpq-connect.html#LIBPQ-CONNSTRING-URIS
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionString(url::Url);

impl ConnectionString {
    /// Create a new connection string from a string.
    /// This function validates that the connection string is a postgres connection string.
    pub fn new(con_string: &str) -> anyhow::Result<Self> {
        let con = Self(url::Url::parse(con_string)?);
        if !con.is_postgres() {
            anyhow::bail!("Only postgres database urls are supported");
        }
        Ok(con)
    }

    /// Get the connection string as a str.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn is_postgres(&self) -> bool {
        self.0.scheme() == "postgres" || self.0.scheme() == "postgresql"
    }

    /// Get the database name
    /// For postgres, this is the database name directly
    /// For sqlite, this is the path to the database file
    pub fn database_name(&self) -> &str {
        self.0.path().trim_start_matches("/")
    }

    /// Set the database name
    pub fn set_database_name(&mut self, db_name: &str) {
        self.0.set_path(db_name);
    }
}

#[cfg(any(test, feature = "testing"))]
impl ConnectionString {
    const TEST_PARAM_KEY: &'static str = "pubky-test";
    const TEST_DB_NAME_KEY: &'static str = "pubky-test-db-name";
    const TEST_PERSIST_KEY: &'static str = "pubky-test-persist";
    const DEFAULT_CONNECTION_STRING: &'static str =
        "postgres://localhost:5432/postgres?pubky-test=true";

    /// Returns a connection string for a test database.
    /// This is a postgres database that is not real.
    /// It is used as an indicator for a empheral test database.
    pub fn default_test_db() -> Self {
        Self::new(Self::DEFAULT_CONNECTION_STRING).unwrap()
    }

    /// Returns true if the connection string is for a test database.
    pub fn is_test_db(&self) -> bool {
        self.has_param(Self::TEST_PARAM_KEY, Some("true"))
    }

    /// Adds a parameter to the connection string that indicates that this is a test database.
    pub fn add_test_db_flag(&mut self) {
        if !self.is_test_db() {
            self.add_param(Self::TEST_PARAM_KEY, "true");
        }
    }

    /// Returns the value of the database name parameter if it exists.
    pub fn db_name_key(&self) -> Option<String> {
        self.0.query_pairs().find_map(|(key, value)| {
            if key == Self::TEST_DB_NAME_KEY {
                Some(value.into_owned())
            } else {
                None
            }
        })
    }

    /// Adds a db name as a parameter to the connection string.
    pub fn add_test_db_name(&mut self, db_name: &str) {
        if self.db_name_key().is_none() {
            self.add_param(Self::TEST_DB_NAME_KEY, db_name);
        }
    }

    /// Removes the db name parameter from the connection string.
    pub fn remove_test_db_name(&mut self) {
        self.remove_param(Self::TEST_DB_NAME_KEY);
    }

    /// Returns true if the connection string is for a persistent database.
    pub fn is_persistent(&self) -> bool {
        self.has_param(Self::TEST_PERSIST_KEY, Some("true"))
    }

    /// Adds a parameter to the connection string that indicates that this db must persist.
    pub fn add_persist_param(&mut self) {
        if !self.is_persistent() {
            self.add_param(Self::TEST_PERSIST_KEY, "true");
        }
    }

    /// Removes the parameter from the connection string that indicates that this db must persist.
    pub fn remove_persist_param(&mut self) {
        self.remove_param(Self::TEST_PERSIST_KEY);
    }

    fn has_param(&self, param_key: &str, param_value: Option<&str>) -> bool {
        self.0.query_pairs().any(|(key, value)| {
            if key == param_key {
                if let Some(param_value) = param_value {
                    return value == param_value;
                }

                return true;
            }

            false
        })
    }

    fn add_param(&mut self, key: &str, value: &str) {
        self.0.query_pairs_mut().append_pair(key, value);
    }

    fn remove_param(&mut self, param_key: &str) {
        let new_queries = self
            .0
            .query_pairs()
            .filter_map(|(key, value)| {
                (key != param_key).then_some((key.into_owned(), value.into_owned()))
            })
            .collect::<Vec<_>>();

        self.0.query_pairs_mut().clear().extend_pairs(new_queries);
    }
}

impl From<url::Url> for ConnectionString {
    fn from(url: url::Url) -> Self {
        Self(url)
    }
}

impl FromStr for ConnectionString {
    type Err = url::ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(url::Url::parse(s)?))
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

impl Default for ConnectionString {
    fn default() -> Self {
        Self::new("postgres://localhost:5432/pubky_homeserver").unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_db() {
        let con_strings = vec![
            "postgres://localhost:5432/pubky_homeserver",
            "sqlite:///path/to/sqlite.db",
        ];
        for con_string in con_strings {
            let _: ConnectionString = con_string.parse().unwrap();
        }
    }
}
