use std::sync::Arc;

use sea_query::{PostgresQueryBuilder, QueryBuilder, SchemaBuilder, SqliteQueryBuilder};
use sqlx::{any::install_default_drivers, AnyPool, Transaction};
#[cfg(test)]
use tempfile::TempDir;

#[derive(Debug, Clone, PartialEq)]
pub enum DbBackend {
    Postgres,
    Sqlite,
}


#[derive(Clone)]
pub struct DbConnection {
    /// Connection pool to the database
    pool: sqlx::Pool<sqlx::Any>,
    /// Database backend type (postgres, sqlite, mysql, etc.)
    database_type: DbBackend,

    /// Schema builder for the database backend
    /// Used to build schema statements
    schema_builder: Arc<Box<dyn SchemaBuilder + Send + Sync>>,

    /// Query builder for the database backend
    /// Used to build query statements
    query_builder: Arc<Box<dyn QueryBuilder + Send + Sync>>,

    /// Temporary directory for sqlite test database
    /// As soon as temp_dir goes out of scope, the database is deleted
    #[cfg(test)]
    temp_dir: Arc<Option<TempDir>>,
}

impl DbConnection {
    pub async fn new(con_string: &str) -> anyhow::Result<Self> {
        install_default_drivers();
        let pool: sqlx::Pool<sqlx::Any> = AnyPool::connect(con_string).await?;
        let database_type = Self::detect_database_type_from_connection_string(con_string)?;
        let schema_builder: Box<dyn SchemaBuilder + Send + Sync> = match database_type {
            DbBackend::Postgres => Box::new(PostgresQueryBuilder::default()),
            DbBackend::Sqlite => Box::new(SqliteQueryBuilder::default()),
        };
        let query_builder: Box<dyn QueryBuilder + Send + Sync> = match database_type {
            DbBackend::Postgres => Box::new(PostgresQueryBuilder::default()),
            DbBackend::Sqlite => Box::new(SqliteQueryBuilder::default()),
        };

        Ok(Self {
            pool,
            database_type,
            schema_builder: Arc::new(schema_builder),
            query_builder: Arc::new(query_builder),
            #[cfg(test)]
            temp_dir: Arc::new(None),
        })
    }

    /// Detect database type from connection string
    fn detect_database_type_from_connection_string(con_string: &str) -> anyhow::Result<DbBackend> {
        if con_string.starts_with("postgres://") || con_string.starts_with("postgresql://") {
            Ok(DbBackend::Postgres)
        } else if con_string.starts_with("sqlite://") || con_string.starts_with("sqlite:") {
            Ok(DbBackend::Sqlite)
        } else {
            Err(anyhow::anyhow!("Unsupported database type"))
        }
    }

    /// Get the database backend type
    pub fn backend(&self) -> DbBackend {
        self.database_type.clone()
    }

    /// Get the connection pool
    pub fn pool(&self) -> &sqlx::Pool<sqlx::Any> {
        &self.pool
    }

    /// Get the query builder for the database backend
    pub fn query_builder(&self) -> &dyn QueryBuilder {
        &**self.query_builder
    }

    /// Get the schema builder for the database backend
    pub fn schema_builder(&self) -> &dyn SchemaBuilder {
        &**self.schema_builder
    }

    pub async fn create_database(&self) -> anyhow::Result<()> {
        // Use different CREATE DATABASE syntax based on backend
        match self.database_type {
            DbBackend::Postgres => {
                let query = "CREATE DATABASE IF NOT EXISTS pubky_homeserver";
                sqlx::query(query).execute(&self.pool).await?;
            }
            DbBackend::Sqlite => {} // Do nothing. Sqlite only has one database anyway.
        };

        Ok(())
    }
}

#[cfg(test)]
impl DbConnection {
    pub async fn test_without_migrations() -> Self {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("sqlite.db");
        std::fs::write(&path, "").unwrap();
        let con_string = format!("sqlite://{}", path.display());
        let mut db = DbConnection::new(&con_string)
            .await
            .expect("Failed to create test database");
        db.temp_dir = Arc::new(Some(temp_dir));
        db
    }

    pub async fn test() -> Self {
        use crate::persistence::sql::migrator::Migrator;
        let db = Self::test_without_migrations().await;
        let migrator = Migrator::new(&db);
        migrator.run().await.expect("Failed to run migrations");
        db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_db_connection() {
        let db = DbConnection::test_without_migrations().await;
        assert_eq!(db.backend(), DbBackend::Sqlite);
    }
}
