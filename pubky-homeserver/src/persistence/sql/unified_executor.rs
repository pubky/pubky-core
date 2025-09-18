use futures_util::future::BoxFuture;

/// Macro to create a unified executor from a transaction.
/// This is a shortcut for `&mut (&mut tx).into()`.
///
/// # Example
/// ```
/// let mut tx = db.pool().begin().await?;
/// let mut executor = uexecutor!(tx);
/// ```
///
macro_rules! uexecutor {
    ($tx:expr) => {{
        &mut crate::persistence::sql::UnifiedExecutor::from_tx(&mut $tx)
    }};
}

// Re-export the macro so it can be imported from this module path
pub(crate) use uexecutor;

/// A unified executor that can be used to execute queries on a pool or a transaction.
/// A sqlx Executor is onetime use only which is restricting. This wrapper allows to use the same executor multiple times.
///
/// Can easily be converted from a pool or a transaction:
/// - `db.pool().into()`
/// - `transaction.into()`
pub(crate) enum UnifiedExecutor<'a> {
    Pool {
        future: BoxFuture<'a, Result<sqlx::pool::PoolConnection<sqlx::Postgres>, sqlx::Error>>,
        connection: Option<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    },
    Transaction(&'a mut sqlx::Transaction<'static, sqlx::Postgres>),
}

impl<'a> UnifiedExecutor<'a> {
    /// Create a new executor from a pool.
    pub fn from_pool(pool: &'a sqlx::PgPool) -> Self {
        let future: BoxFuture<'a, Result<sqlx::pool::PoolConnection<sqlx::Postgres>, sqlx::Error>> =
            Box::pin(async move { pool.acquire().await });
        UnifiedExecutor::Pool {
            future,
            connection: None,
        }
    }

    /// Create a new executor from a transaction.
    pub fn from_tx(tx: &'a mut sqlx::Transaction<'static, sqlx::Postgres>) -> Self {
        UnifiedExecutor::Transaction(tx)
    }

    /// Get the connection from the executor.
    /// If the executor is a pool, it will acquire a connection from the pool.
    /// If the executor is a transaction, it will return the transaction.
    pub async fn get_con(&mut self) -> Result<&mut sqlx::PgConnection, sqlx::Error> {
        match self {
            UnifiedExecutor::Pool { future, connection } => {
                if connection.is_none() {
                    // Store the connection so we can return a reference
                    let con = future.await?;
                    *connection = Some(con);
                }

                Ok(connection.as_mut().expect("Connection should be present"))
            }
            UnifiedExecutor::Transaction(tx) => Ok(&mut **tx),
        }
    }
}

impl<'a> From<&'a sqlx::PgPool> for UnifiedExecutor<'a> {
    fn from(pool: &'a sqlx::PgPool) -> Self {
        UnifiedExecutor::from_pool(pool)
    }
}

impl<'a> From<&'a mut sqlx::Transaction<'static, sqlx::Postgres>> for UnifiedExecutor<'a> {
    fn from(tx: &'a mut sqlx::Transaction<'static, sqlx::Postgres>) -> Self {
        UnifiedExecutor::from_tx(tx)
    }
}

impl std::fmt::Debug for UnifiedExecutor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnifiedExecutor")
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence::sql::SqlDb;

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_executor_holder_from_pool() {
        let db = SqlDb::test().await;
        let mut holder = UnifiedExecutor::from_pool(db.pool());
        let _con = holder
            .get_con()
            .await
            .expect("Should be able to get connection");

        let _holder: UnifiedExecutor<'_> = db.pool().into();
        let _con = holder
            .get_con()
            .await
            .expect("Should be able to get connection");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_executor_holder_from_tx() {
        let db = SqlDb::test().await;
        let mut tx = db
            .pool()
            .begin()
            .await
            .expect("Should be able to begin transaction");
        {
            let mut holder = UnifiedExecutor::from_tx(&mut tx);
            let _con = holder
                .get_con()
                .await
                .expect("Should be able to get connection");
        }
        let mut holder: UnifiedExecutor<'_> = (&mut tx).into();
        let _con = holder
            .get_con()
            .await
            .expect("Should be able to get connection");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_executor_holder_from_tx_macro() {
        let db = SqlDb::test().await;
        let mut tx = db
            .pool()
            .begin()
            .await
            .expect("Should be able to begin transaction");
        {
            let holder = uexecutor!(tx);
            let _con = holder
                .get_con()
                .await
                .expect("Should be able to get connection");
        }
        let holder = uexecutor!(tx);
        let _con = holder
            .get_con()
            .await
            .expect("Should be able to get connection");
    }
}
