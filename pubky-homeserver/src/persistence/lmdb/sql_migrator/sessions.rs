use pubky_common::session::Session;
use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr, Value};
use sea_query_binder::SqlxBinder;
use sqlx::types::chrono::NaiveDateTime;

use crate::persistence::{lmdb::{LmDB}, sql::{session::{SessionIden, SESSION_TABLE}, user::UserRepository, SqlDb, UnifiedExecutor}};


    /// Create a new signup code.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(session_secret: &str, lmdb_session: &Session, executor: &mut UnifiedExecutor<'a>) -> Result<(), sqlx::Error> {
        let sql_user = UserRepository::get(lmdb_session.pubky(), executor).await?;
        let created_at = NaiveDateTime::from_timestamp(lmdb_session.created_at() as i64, 0);
        let statement =
        Query::insert().into_table(SESSION_TABLE)
            .columns([SessionIden::Secret, SessionIden::User, SessionIden::Capabilities, SessionIden::CreatedAt])
            .values(vec![
                SimpleExpr::Value(session_secret.into()),
                SimpleExpr::Value(sql_user.id.into()),
                SimpleExpr::Value(lmdb_session.capabilities().iter().map(|c| c.to_string()).collect::<Vec<String>>().into()),
                SimpleExpr::Value(created_at.into()),
            ]).expect("Failed to build insert statement").to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder::default());

        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }


pub async fn migrate_sessions(lmdb: &LmDB, sql_db: &SqlDb) -> anyhow::Result<()> {
    tracing::info!("Migrating sessions from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut sql_tx = sql_db.pool().begin().await?;
    let mut count = 0;
    for record in lmdb.tables.sessions.iter(&lmdb_txn)? {
            let (secret, bytes) = record?;
            let session = Session::deserialize(&bytes)?;
            create(secret, &session,  &mut(&mut sql_tx).into()).await?;
        count += 1;
    }
    sql_tx.commit().await?;
    tracing::info!("Migrated {} sessions", count);
    Ok(())
}


#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pkarr::Keypair;
    use pubky_common::capabilities::Capability;
    use sqlx::types::chrono::DateTime;

    use crate::persistence::sql::{session::{SessionRepository, SessionSecret}};

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();
        // Session1
        let session1_secret = SessionSecret::random();
        let user1_pubkey = Keypair::random().public_key();
        UserRepository::create(&user1_pubkey, &mut sql_db.pool().into()).await.unwrap();
        let created_at1 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut session1 = Session::new(&user1_pubkey, &[Capability::root()], None);
        session1.set_created_at(created_at1);
        lmdb.tables.sessions.put(&mut wtxn, &session1_secret.to_string(), &session1.serialize()).unwrap();


        // Session2
        let session2_secret = SessionSecret::random();
        let user2_pubkey = Keypair::random().public_key();
        UserRepository::create(&user2_pubkey, &mut sql_db.pool().into()).await.unwrap();
        let created_at2 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut session2 = Session::new(&user2_pubkey, &[], None);
        session2.set_created_at(created_at2);
        lmdb.tables.sessions.put(&mut wtxn, &session2_secret.to_string(), &session2.serialize()).unwrap();

        wtxn.commit().unwrap();

        // Migrate
        migrate_sessions(&lmdb, &sql_db).await.unwrap();

        // Check
        let sql_session1 = SessionRepository::get_by_secret(&session1_secret, &mut sql_db.pool().into()).await.unwrap();
        assert_eq!(sql_session1.created_at.format("%Y-%m-%d %H:%M:%S").to_string(), DateTime::from_timestamp(created_at1 as i64, 0).unwrap().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string());
        assert_eq!(sql_session1.user_pubkey, user1_pubkey);
        assert_eq!(sql_session1.capabilities, vec![Capability::root()]);
        assert_eq!(sql_session1.user_pubkey, user1_pubkey);
        assert_eq!(sql_session1.secret, session1_secret);

        let sql_session2 = SessionRepository::get_by_secret(&session2_secret, &mut sql_db.pool().into()).await.unwrap();
        assert_eq!(sql_session2.created_at.format("%Y-%m-%d %H:%M:%S").to_string(), DateTime::from_timestamp(created_at2 as i64, 0).unwrap().naive_utc().format("%Y-%m-%d %H:%M:%S").to_string());
        assert_eq!(sql_session2.user_pubkey, user2_pubkey);
        assert_eq!(sql_session2.capabilities, vec![]);
        assert_eq!(sql_session2.user_pubkey, user2_pubkey);
        assert_eq!(sql_session2.secret, session2_secret);
    }
}