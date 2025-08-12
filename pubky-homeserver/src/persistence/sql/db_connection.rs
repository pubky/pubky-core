#[cfg(test)]
use async_dropper::AsyncDrop;
#[cfg(test)]
use async_dropper::AsyncDropper;
#[cfg(test)]
use async_trait::async_trait;
use sea_query::{
    PostgresQueryBuilder, QueryBuilder, SchemaBuilder, SchemaStatementBuilder, SqliteQueryBuilder,
};
use sea_query_binder::{SqlxBinder, SqlxValues};
use sqlx::{any::install_default_drivers, AnyPool};
use std::sync::Arc;
#[cfg(test)]
use tempfile::TempDir;

use crate::persistence::sql::connection_string::{ConnectionString, DbBackend};


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

    /// Test helper for postgres to drop the test database after the test
    #[cfg(test)]
    drop_pg_db_after_test: Option<Arc<AsyncDropper<DropPgDbAfterTest>>>,
}

impl DbConnection {
    pub async fn new(con_string: &ConnectionString) -> anyhow::Result<Self> {
        install_default_drivers();
        if con_string.backend() == DbBackend::Sqlite {
            Self::create_sqlite_db_if_not_exists(con_string)?;
        }
        let pool: sqlx::Pool<sqlx::Any> = AnyPool::connect(con_string.as_str()).await?;
        let schema_builder: Box<dyn SchemaBuilder + Send + Sync> = match con_string.backend() {
            DbBackend::Postgres => Box::new(PostgresQueryBuilder::default()),
            DbBackend::Sqlite => Box::new(SqliteQueryBuilder::default()),
        };
        let query_builder: Box<dyn QueryBuilder + Send + Sync> = match con_string.backend() {
            DbBackend::Postgres => Box::new(PostgresQueryBuilder::default()),
            DbBackend::Sqlite => Box::new(SqliteQueryBuilder::default()),
        };

        Ok(Self {
            pool,
            database_type: con_string.backend(),
            schema_builder: Arc::new(schema_builder),
            query_builder: Arc::new(query_builder),
            #[cfg(test)]
            temp_dir: Arc::new(None),
            #[cfg(test)]
            drop_pg_db_after_test: None,
        })
    }

    /// Creates the sqlite database file if it does not exist
    fn create_sqlite_db_if_not_exists(con_string: &ConnectionString) -> anyhow::Result<()> {
        if !std::fs::exists(con_string.database_name())? {
            tracing::info!(
                "Creating sqlite database file at {}",
                con_string.database_name()
            );
            std::fs::write(con_string.database_name(), "")?;
        }

        Ok(())
    }

    /// Get the database backend type
    pub fn backend(&self) -> DbBackend {
        self.database_type.clone()
    }

    /// Get the connection pool
    pub fn pool(&self) -> &sqlx::Pool<sqlx::Any> {
        &self.pool
    }

    /// Build a query with the db backend specific query builder
    pub fn build_query<S>(&self, statement: S) -> (String, SqlxValues)
    where
        S: SqlxBinder,
    {
        let (query, values) = statement.build_any_sqlx(&**self.query_builder);
        (query, values)
    }

    /// Build a schema with the db backend specific schema builder
    pub fn build_schema<S>(&self, statement: S) -> String
    where
        S: SchemaStatementBuilder,
    {
        statement.build_any(&**self.schema_builder)
    }
}

/// Helper struct to drop the postgres test database after the db connection is dropped
/// Important: This requires the tokio::test(flavor = "multi_thread") attribute,
/// Otherwise the test will panic when the db connection is dropped
#[cfg(test)]
#[derive(Default)]
struct DropPgDbAfterTest{
    db_name: String,
    pool: Option<sqlx::Pool<sqlx::Any>>,
}
#[cfg(test)]
#[async_trait]
impl AsyncDrop for DropPgDbAfterTest {
    async fn async_drop(&mut self) {
        let pool = match &self.pool {
            Some(pool) => pool,
            None => return,
        };
        let query = format!("DROP DATABASE {}", self.db_name);
        if let Err(e) = sqlx::query(&query).execute(pool).await {
            println!("Failed to drop test database {}: {}", query, e);
        }
    }
}


#[cfg(test)]
impl DbConnection {

    pub async fn test_sqlite_db() -> anyhow::Result<Self> {
        let temp_dir = tempfile::tempdir()?;
        let path = temp_dir.path().join("sqlite.db");
        let con_string = format!("sqlite://{}", path.display());
        let mut db = Self::new(&ConnectionString::new(&con_string)?).await?;
        db.temp_dir = Arc::new(Some(temp_dir));
        Ok(db)
    }

    pub async fn test_postgres_db(con_string: &ConnectionString) -> anyhow::Result<Self> {
        use uuid::Uuid;

        assert_eq!(con_string.backend(), DbBackend::Postgres);
        let neutral_con = Self::new(&con_string).await?;
        let db_name = format!("pubky_test_{}", Uuid::new_v4().as_simple());
        let query = format!("CREATE DATABASE {}", db_name);

        sqlx::query(&query).execute(neutral_con.pool()).await?;
        let mut con_string = con_string.clone();
        con_string.set_database_name(&db_name);
        let mut con = Self::new(&con_string).await?;
        con.drop_pg_db_after_test = Some(Arc::new(AsyncDropper::new(DropPgDbAfterTest {
            db_name,
            pool: Some(neutral_con.pool().clone()),
        })));
        Ok(con)
    }

    fn con_string_from_pg_test_env_var() -> Option<ConnectionString> {
        let raw_con_string = std::env::var("TEST_PG_CONNECTION_STRING").ok()?;
        ConnectionString::new(&raw_con_string).ok()
    }

    /// Create a test database without running migrations
    /// If the DB_CONNECTION_STRING environment variable is not set, a temporary directory is used for the sqlite database
    /// If the DB_CONNECTION_STRING environment variable is set, the test database is created on the existing database
    pub async fn test_without_migrations() -> Self {
        match Self::con_string_from_pg_test_env_var() {
            Some(con_string) => Self::test_postgres_db(&con_string).await.unwrap(),
            None => Self::test_sqlite_db().await.unwrap(),
        }
    }

    /// Create a test database and run migrations
    /// If the DB_CONNECTION_STRING environment variable is not set, a temporary directory is used for the sqlite database
    /// If the DB_CONNECTION_STRING environment variable is set, the migrations are run on the existing database
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
    use crate::persistence::sql::{connection_string::DbBackend};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_sqlite_db() {
        let db = DbConnection::test_sqlite_db().await.unwrap();
        assert_eq!(db.backend(), DbBackend::Sqlite);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_pg_db() {
        let db = DbConnection::test_postgres_db(&ConnectionString::new("postgres://localhost:5432/postgres").unwrap()).await.unwrap();
        assert_eq!(db.backend(), DbBackend::Postgres);
    }
}


mod test {

}