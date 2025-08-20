use futures_util::future::BoxFuture;

/// A unified executor that can be used to execute queries on a pool or a transaction.
/// a sqlx Executor is onetime use only which restricting. This wrapper allows to use the same executor multiple times.
/// 
/// Can easily be converted from a pool or a transaction:
/// - `db.pool().into()`;
/// - `transaction.into()`;
pub enum UnfiedExecutor<'a> {
    Pool{
        future: BoxFuture<'a, Result<sqlx::pool::PoolConnection<sqlx::Postgres>, sqlx::Error>>,
        connection: Option<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    },
    Transaction(&'a sqlx::Transaction<'static, sqlx::Postgres>),
}

impl<'a> UnfiedExecutor<'a> {
    /// Create a new executor from a pool.
    pub fn from_pool(pool: &'a sqlx::PgPool) -> Self {
        let future: BoxFuture<'a, Result<sqlx::pool::PoolConnection<sqlx::Postgres>, sqlx::Error>> = Box::pin(async move { pool.acquire().await });
        UnfiedExecutor::Pool{future, connection: None}
    }

    /// Create a new executor from a transaction.
    pub fn from_tx(tx: &'a sqlx::Transaction<'static, sqlx::Postgres>) -> Self {
        UnfiedExecutor::Transaction(tx)
    }

    /// Get the connection from the executor.
    /// If the executor is a pool, it will acquire a connection from the pool.
    /// If the executor is a transaction, it will return the transaction.
    pub async fn get_con(&mut self) -> Result<&sqlx::PgConnection, sqlx::Error> {
        match self {
            UnfiedExecutor::Pool{future, connection} => {
                if let None = connection {
                    // Store the connection so we can return a reference
                    let con = future.await?;
                    *connection = Some(con);
                }

                Ok(connection.as_ref().expect("Connection should be present"))
            },
            UnfiedExecutor::Transaction(tx) => Ok(&***tx),
        }
    }
}

impl<'a> From<&'a sqlx::PgPool> for UnfiedExecutor<'a> {
    fn from(pool: &'a sqlx::PgPool) -> Self {
        UnfiedExecutor::from_pool(pool)
    }
}

impl<'a> From<&'a sqlx::Transaction<'static, sqlx::Postgres>> for UnfiedExecutor<'a> {
    fn from(tx: &'a sqlx::Transaction<'static, sqlx::Postgres>) -> Self {
        UnfiedExecutor::from_tx(tx)
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence::sql::SqlDb;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_executor_holder_from_pool() {
        let db = SqlDb::test().await;
        let mut holder = UnfiedExecutor::from_pool(db.pool());
        let _con = holder.get_con().await.expect("Should be able to get connection");

        let _holder: UnfiedExecutor<'_> = db.pool().into();
        let _con = holder.get_con().await.expect("Should be able to get connection");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_executor_holder_from_tx() {
        let db = SqlDb::test().await;

        let tx = db.pool().begin().await.expect("Should be able to begin transaction");
        let mut holder = UnfiedExecutor::from_tx(&tx);
        let _con = holder.get_con().await.expect("Should be able to get connection");

        let _holder: UnfiedExecutor<'_> = db.pool().into();
        let _con = holder.get_con().await.expect("Should be able to get connection");
    }
}