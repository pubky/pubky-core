//! Repository for PoP nonce replay prevention.

use pubky_common::auth::jws::PopNonce;
use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;

use crate::persistence::sql::{
    migrations::m20260325_create_grant_sessions::{PopNonceIden, POP_NONCES_TABLE},
    UnifiedExecutor,
};

/// Repository for PoP nonce tracking and garbage collection.
pub struct PopNonceRepository;

impl PopNonceRepository {
    /// Attempt to track a nonce. Returns `Ok(())` if the nonce is new.
    ///
    /// Returns a unique constraint violation error if the nonce was already
    /// seen (replay attack detected).
    pub async fn check_and_track<'a>(
        nonce: &PopNonce,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let statement = Query::insert()
            .into_table(POP_NONCES_TABLE)
            .columns([PopNonceIden::Nonce])
            .values(vec![SimpleExpr::Value(nonce.to_string().into())])
            .expect("invariant: values count matches columns count")
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }

    /// Delete nonces older than `max_age_secs` seconds.
    ///
    /// Called lazily during nonce checks to prevent unbounded table growth.
    pub async fn garbage_collect<'a>(
        max_age_secs: u64,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<u64, sqlx::Error> {
        let cutoff = chrono::Utc::now() - chrono::Duration::seconds(max_age_secs as i64);

        let statement = Query::delete()
            .from_table(POP_NONCES_TABLE)
            .and_where(Expr::col(PopNonceIden::CreatedAt).lt(cutoff.naive_utc()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let result = sqlx::query_with(&query, values).execute(con).await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::auth::jws::PopNonce;

    use crate::persistence::sql::SqlDb;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_check_and_track_new_nonce() {
        let db = SqlDb::test().await;
        let nonce = PopNonce::generate();
        PopNonceRepository::check_and_track(&nonce, &mut db.pool().into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_check_and_track_duplicate_rejected() {
        let db = SqlDb::test().await;
        let nonce = PopNonce::generate();

        PopNonceRepository::check_and_track(&nonce, &mut db.pool().into())
            .await
            .unwrap();

        // Duplicate should fail with unique constraint violation
        let result = PopNonceRepository::check_and_track(&nonce, &mut db.pool().into()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.as_database_error().is_some());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_garbage_collect_removes_old_nonces() {
        let db = SqlDb::test().await;
        let nonce = PopNonce::generate();

        PopNonceRepository::check_and_track(&nonce, &mut db.pool().into())
            .await
            .unwrap();

        // GC with max_age=0 should remove everything
        let deleted = PopNonceRepository::garbage_collect(0, &mut db.pool().into())
            .await
            .unwrap();
        assert!(deleted >= 1);

        // Nonce should be gone, so reinserting should succeed
        PopNonceRepository::check_and_track(&nonce, &mut db.pool().into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_garbage_collect_preserves_recent() {
        let db = SqlDb::test().await;
        let nonce = PopNonce::generate();

        PopNonceRepository::check_and_track(&nonce, &mut db.pool().into())
            .await
            .unwrap();

        // GC with 1-hour threshold should keep the just-inserted nonce
        let deleted = PopNonceRepository::garbage_collect(3600, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(deleted, 0);

        // Nonce should still be there, so reinserting should fail
        let result = PopNonceRepository::check_and_track(&nonce, &mut db.pool().into()).await;
        assert!(result.is_err());
    }
}
