use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

/// A connection string for a database
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionString(url::Url);

impl ConnectionString {
    pub fn new(con_string: &str) -> anyhow::Result<Self> {
        let con = Self(url::Url::parse(con_string)?);
        if !con.is_postgres() {
            anyhow::bail!("Only postgres is supported");
        }
        Ok(con)
    }

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

    pub fn set_database_name(&mut self, db_name: &str) {
        self.0.set_path(db_name);
    }
}

#[cfg(any(test, feature = "testing"))]
impl ConnectionString {
    /// Returns a connection string for a test database.
    /// This is a postgres database that is not real.
    /// It is used as an indicator for a empheral test database.
    pub fn test_db() -> Self {
        Self::new("postgres://test_pubky_hs_test:5432/test").unwrap()
    }

    /// Returns true if the connection string is for a test database.
    pub fn is_test_db(&self) -> bool {
        let host = self.0.host().unwrap();
        let host_str = host.to_string();
        host_str.contains("test_pubky_hs_test")
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

    #[tokio::test(flavor = "multi_thread")]
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
