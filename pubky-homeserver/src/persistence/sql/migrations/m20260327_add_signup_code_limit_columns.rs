use async_trait::async_trait;
use sqlx::Transaction;

use crate::persistence::sql::migration::MigrationTrait;

pub struct M20260327AddSignupCodeLimitColumnsMigration;

#[async_trait]
impl MigrationTrait for M20260327AddSignupCodeLimitColumnsMigration {
    async fn up(&self, tx: &mut Transaction<'static, sqlx::Postgres>) -> anyhow::Result<()> {
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_storage_quota_mb BIGINT",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_max_sessions INTEGER",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_rate_read VARCHAR(32)",
        )
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "ALTER TABLE signup_codes ADD COLUMN IF NOT EXISTS limit_rate_write VARCHAR(32)",
        )
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    fn name(&self) -> &str {
        "m20260327_add_signup_code_limit_columns"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::sql::entities::signup_code::{SignupCodeId, SignupCodeIden, SIGNUP_CODE_TABLE};
    use crate::persistence::sql::migrations::{
        M20250806CreateUserMigration, M20250812CreateSignupCodeMigration,
    };
    use crate::persistence::sql::migrator::Migrator;
    use crate::persistence::sql::sql_db::SqlDb;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
    use sea_query_binder::SqlxBinder;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_adds_limit_columns_to_signup_codes() {
        let db = SqlDb::test_without_migrations().await;
        let migrator = Migrator::new(&db, Default::default());
        migrator
            .run_migrations(vec![
                Box::new(M20250806CreateUserMigration),
                Box::new(M20250812CreateSignupCodeMigration),
                Box::new(M20260327AddSignupCodeLimitColumnsMigration),
            ])
            .await
            .expect("migrations should succeed");

        // Insert a signup code
        let code_id = SignupCodeId::random();
        let statement = Query::insert()
            .into_table(SIGNUP_CODE_TABLE)
            .columns([SignupCodeIden::Id])
            .values(vec![SimpleExpr::Value(code_id.to_string().into())])
            .unwrap()
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(query.as_str(), values)
            .execute(db.pool())
            .await
            .unwrap();

        // Verify limit columns exist and default to NULL
        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT limit_storage_quota_mb, limit_max_sessions, limit_rate_read, limit_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(code_id.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row, (None, None, None, None));

        // Verify we can insert a code with limit values
        let code_id2 = SignupCodeId::random();
        sqlx::query(
            "INSERT INTO signup_codes (id, limit_storage_quota_mb, limit_max_sessions, limit_rate_read) VALUES ($1, $2, $3, $4)",
        )
        .bind(code_id2.to_string())
        .bind(1024_i64)
        .bind(5_i32)
        .bind("200mb/m")
        .execute(db.pool())
        .await
        .unwrap();

        let row: (Option<i64>, Option<i32>, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT limit_storage_quota_mb, limit_max_sessions, limit_rate_read, limit_rate_write FROM signup_codes WHERE id = $1",
        )
        .bind(code_id2.to_string())
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(row.0, Some(1024));
        assert_eq!(row.1, Some(5));
        assert_eq!(row.2, Some("200mb/m".to_string()));
        assert_eq!(row.3, None);
    }
}
