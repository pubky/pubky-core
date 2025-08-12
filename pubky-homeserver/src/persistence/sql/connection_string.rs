use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone, PartialEq)]
pub enum DbBackend {
    Postgres,
    Sqlite,
}

/// A connection string for a database
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionString(url::Url);

impl ConnectionString {
    pub fn new(con_string: &str) -> anyhow::Result<Self> {
        Ok(Self(url::Url::parse(con_string)?))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn as_url(&self) -> &url::Url {
        &self.0
    }

    /// Get the database backend type
    pub fn backend(&self) -> DbBackend {
        match self.0.scheme() {
            "postgres" | "postgresql" => DbBackend::Postgres,
            "sqlite" => DbBackend::Sqlite,
            _ => panic!("Unsupported database type"),
        }
    }

    /// Get the database name
    /// For postgres, this is the database name directly
    /// For sqlite, this is the path to the database file
    pub fn database_name(&self) -> &str {
        match self.backend() {
            DbBackend::Postgres => self.0.path().trim_start_matches("/"),
            DbBackend::Sqlite => self.0.path(),
        }
    }

    pub fn set_database_name(&mut self, db_name: &str) {
        self.0.set_path(db_name);
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



#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
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