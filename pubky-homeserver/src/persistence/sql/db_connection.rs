use sqlx::postgres::PgPool;

use crate::persistence::sql::connection_string::ConnectionString;

#[derive(Clone)]
pub struct SqlDb {
    /// Connection pool to the database
    pool: PgPool,
    /// Test helper for postgres to drop the test database after the test
    #[cfg(any(test, feature = "testing"))]
    db_dropper: Option<std::sync::Arc<TestDbDropper>>,
}

impl std::fmt::Debug for SqlDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DbConnection")
    }
}

impl SqlDb {
    pub async fn connect(con_string: &ConnectionString) -> Result<Self, sqlx::Error> {
        let pool: PgPool = PgPool::connect(con_string.as_str()).await?;

        Ok(Self {
            pool,
            #[cfg(any(test, feature = "testing"))]
            db_dropper: None,
        })
    }

    /// Get the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

/// Helper struct to drop the postgres test database after the db connection is dropped.
#[cfg(any(test, feature = "testing"))]
struct TestDbDropper {
    db_name: String,
    connection_string: String,
}

#[cfg(any(test, feature = "testing"))]
impl TestDbDropper {
    pub fn new(db_name: String, connection_string: String) -> Self {
        Self {
            db_name,
            connection_string,
        }
    }
}

#[cfg(any(test, feature = "testing"))]
impl Drop for TestDbDropper {
    fn drop(&mut self) {
        // Drop the database after the test.
        // This works in combination with the pubky_test macro.
        let _ = pubky_test_utils::register_db_to_drop(
            self.db_name.clone(),
            self.connection_string.clone(),
        );
    }
}

#[cfg(any(test, feature = "testing"))]
const DEFAULT_TEST_CONNECTION_STRING: &str = "postgres://localhost:5432/postgres";

#[cfg(any(test, feature = "testing"))]
impl SqlDb {
    pub async fn test_postgres_db(con_string: &ConnectionString) -> anyhow::Result<Self> {
        use uuid::Uuid;

        let neutral_con = Self::connect(con_string).await?;
        let db_name = format!("pubky_test_{}", Uuid::new_v4().as_simple());
        let query = format!("CREATE DATABASE {}", db_name);

        sqlx::query(&query).execute(neutral_con.pool()).await?;
        let mut con_string = con_string.clone();
        con_string.set_database_name(&db_name);
        let mut con = Self::connect(&con_string).await?;
        con.db_dropper = Some(std::sync::Arc::new(TestDbDropper::new(
            db_name,
            con_string.to_string(),
        )));
        Ok(con)
    }

    pub fn con_string_from_pg_test_env_var() -> ConnectionString {
        match std::env::var("TEST_PG_CONNECTION_STRING") {
            Ok(raw_con_string) => ConnectionString::new(&raw_con_string).unwrap(),
            Err(_) => ConnectionString::new(DEFAULT_TEST_CONNECTION_STRING).unwrap(),
        }
    }

    /// Create a test database without running migrations
    /// If the DB_CONNECTION_STRING environment variable is not set, a temporary directory is used for the sqlite database
    /// If the DB_CONNECTION_STRING environment variable is set, the test database is created on the existing database
    pub async fn test_without_migrations() -> Self {
        Self::test_postgres_db(&Self::con_string_from_pg_test_env_var())
            .await
            .unwrap()
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
    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_pg_db_available() {
        let _db = SqlDb::test_postgres_db(&SqlDb::con_string_from_pg_test_env_var())
            .await
            .unwrap();
    }
}
