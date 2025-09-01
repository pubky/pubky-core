use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr, Value};
use sea_query_binder::SqlxBinder;
use sqlx::types::chrono::NaiveDateTime;

use crate::persistence::{lmdb::{tables::signup_tokens::SignupToken, LmDB}, sql::{signup_code::{SignupCodeIden, SIGNUP_CODE_TABLE}, SqlDb, UnifiedExecutor}};


    /// Create a new signup code.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(lmdb_token: &SignupToken, executor: &mut UnifiedExecutor<'a>) -> Result<(), sqlx::Error> {

        let used_by = match lmdb_token.used.as_ref() {
            Some(p) => SimpleExpr::Value(p.to_string().into()),
            None => SimpleExpr::Value(Value::String(None))
        };
        let created_at = NaiveDateTime::from_timestamp(lmdb_token.created_at as i64, 0);
        let statement =
        Query::insert().into_table(SIGNUP_CODE_TABLE)
            .columns([SignupCodeIden::Id, SignupCodeIden::CreatedAt, SignupCodeIden::UsedBy])
            .values(vec![
                SimpleExpr::Value(lmdb_token.token.clone().into()),
                SimpleExpr::Value(created_at.into()),
                used_by
            ]).expect("Should be valid values").to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder::default());
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }


pub async fn migrate_signup_codes(lmdb: &LmDB, sql_db: &SqlDb) -> anyhow::Result<()> {
    tracing::info!("Migrating signup codes from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut sql_tx = sql_db.pool().begin().await?;
    let mut count = 0;
    for record in lmdb.tables.signup_tokens.iter(&lmdb_txn)? {
            let (_, bytes) = record?;
            let token = SignupToken::deserialize(&bytes);
            create(&token,  &mut(&mut sql_tx).into()).await?;
        count += 1;
    }
    sql_tx.commit().await?;
    tracing::info!("Migrated {} signup codes", count);
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pkarr::Keypair;
    use sqlx::types::chrono::DateTime;

    use crate::persistence::sql::signup_code::{SignupCodeId, SignupCodeRepository};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();
        // Token1
        let mut token1 = SignupToken::random();
        let user1_pubkey = Keypair::random().public_key();
        token1.created_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        token1.used = Some(user1_pubkey.clone());
        lmdb.tables.signup_tokens.put(&mut wtxn, &token1.token, &token1.serialize()).unwrap();

        // Token2
        let mut token2 = SignupToken::random();
        token2.created_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        token2.used = None;
        lmdb.tables.signup_tokens.put(&mut wtxn, &token2.token, &token2.serialize()).unwrap();

        wtxn.commit().unwrap();

        // Migrate
        migrate_signup_codes(&lmdb, &sql_db).await.unwrap();

        // Check
        let id1 = SignupCodeId::new(token1.token).unwrap();
        let sql_code1 = SignupCodeRepository::get(&id1, &mut sql_db.pool().into()).await.unwrap();
        assert_eq!(sql_code1.created_at.format("%Y-%m-%d %H:%M:%S").to_string(), DateTime::from_timestamp(token1.created_at as i64, 0).unwrap().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string());
        assert_eq!(sql_code1.used_by, Some(user1_pubkey));

        let id2 = SignupCodeId::new(token2.token).unwrap();
        let sql_code2 = SignupCodeRepository::get(&id2, &mut sql_db.pool().into()).await.unwrap();
        assert_eq!(sql_code2.created_at.format("%Y-%m-%d %H:%M:%S").to_string(), DateTime::from_timestamp(token2.created_at as i64, 0).unwrap().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string());
        assert_eq!(sql_code2.used_by, None);
    }
}