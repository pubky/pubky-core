use pubky_common::{capabilities::Capabilities, session::SessionInfo};
use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;

use crate::persistence::{
    lmdb::{sql_migrator::users::nano_seconds_to_timestamp, LmDB},
    sql::{
        session::{SessionIden, SESSION_TABLE},
        user::UserRepository,
        UnifiedExecutor,
    },
};

/// Create a new signup code.
/// The executor can either be db.pool() or a transaction.
pub async fn create<'a>(
    session_secret: &str,
    lmdb_session: &SessionInfo,
    executor: &mut UnifiedExecutor<'a>,
) -> Result<(), sqlx::Error> {
    let sql_user = UserRepository::get(lmdb_session.public_key(), executor).await?;
    let created_at =
        nano_seconds_to_timestamp(lmdb_session.created_at()).expect("Should always be valid");
    let created_at = created_at.naive_utc();
    let statement = Query::insert()
        .into_table(SESSION_TABLE)
        .columns([
            SessionIden::Secret,
            SessionIden::User,
            SessionIden::Capabilities,
            SessionIden::CreatedAt,
        ])
        .values(vec![
            SimpleExpr::Value(session_secret.into()),
            SimpleExpr::Value(sql_user.id.into()),
            SimpleExpr::Value(
                Capabilities::from(lmdb_session.capabilities().to_vec())
                    .to_string()
                    .into(),
            ),
            SimpleExpr::Value(created_at.into()),
        ])
        .expect("Failed to build insert statement")
        .to_owned();

    let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

    let con = executor.get_con().await?;
    sqlx::query_with(&query, values).execute(con).await?;
    Ok(())
}

pub async fn migrate_sessions<'a>(
    lmdb: LmDB,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    tracing::info!("Migrating sessions from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut count = 0;
    for record in lmdb.tables.sessions.iter(&lmdb_txn)? {
        let (secret, bytes) = record?;
        let session = SessionInfo::deserialize(bytes)?;
        create(secret, &session, executor).await?;
        count += 1;
    }
    tracing::info!("Migrated {} sessions", count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pubky_common::capabilities::{Capabilities, Capability};
    use pubky_common::crypto::Keypair;

    use crate::persistence::sql::{
        session::{SessionRepository, SessionSecret},
        SqlDb,
    };

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();

        // Session1
        let session1_secret = SessionSecret::random();
        let user1_pubkey = Keypair::random().public_key();
        UserRepository::create(&user1_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        let created_at1 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            * 1_000_000;
        let mut session1 = SessionInfo::new(
            &user1_pubkey,
            Capabilities::builder().cap(Capability::root()).finish(),
            None,
        );
        session1.set_created_at(created_at1);
        lmdb.tables
            .sessions
            .put(
                &mut wtxn,
                &session1_secret.to_string(),
                &session1.serialize(),
            )
            .unwrap();

        // Session2
        let session2_secret = SessionSecret::random();
        let user2_pubkey = Keypair::random().public_key();
        UserRepository::create(&user2_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        let created_at2 = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            * 1_000_000;
        let mut session2 = SessionInfo::new(&user2_pubkey, Capabilities::builder().finish(), None);
        session2.set_created_at(created_at2);
        lmdb.tables
            .sessions
            .put(
                &mut wtxn,
                &session2_secret.to_string(),
                &session2.serialize(),
            )
            .unwrap();

        wtxn.commit().unwrap();

        // Migrate
        migrate_sessions(lmdb.clone(), &mut sql_db.pool().into())
            .await
            .unwrap();

        // Check
        let sql_session1 =
            SessionRepository::get_by_secret(&session1_secret, &mut sql_db.pool().into())
                .await
                .unwrap();
        assert_eq!(
            sql_session1
                .created_at
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            nano_seconds_to_timestamp(created_at1)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(sql_session1.user_pubkey, user1_pubkey);
        assert_eq!(
            sql_session1.capabilities,
            Capabilities::builder().cap(Capability::root()).finish()
        );
        assert_eq!(sql_session1.user_pubkey, user1_pubkey);
        assert_eq!(sql_session1.secret, session1_secret);

        let sql_session2 =
            SessionRepository::get_by_secret(&session2_secret, &mut sql_db.pool().into())
                .await
                .unwrap();
        assert_eq!(
            sql_session2
                .created_at
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            nano_seconds_to_timestamp(created_at2)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(sql_session2.user_pubkey, user2_pubkey);
        assert_eq!(sql_session2.capabilities, Capabilities::builder().finish());
        assert_eq!(sql_session2.user_pubkey, user2_pubkey);
        assert_eq!(sql_session2.secret, session2_secret);
    }
}
