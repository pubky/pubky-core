use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::types::chrono::{DateTime, Utc};

use crate::persistence::{
    lmdb::LmDB,
    sql::{
        user::{UserEntity, UserIden, UserRepository, USER_TABLE},
        UnifiedExecutor,
    },
};
use pubky_common::crypto::PublicKey;

/// Convert nano seconds to a timestamp.
pub fn nano_seconds_to_timestamp(nano_seconds: u64) -> Option<DateTime<Utc>> {
    let ns = nano_seconds % 1_000_000;
    let secs = nano_seconds / 1_000_000;
    DateTime::from_timestamp(secs as i64, ns as u32)
}

pub async fn update_user<'a>(
    user: &UserEntity,
    executor: &mut UnifiedExecutor<'a>,
) -> Result<UserEntity, sqlx::Error> {
    let statement = Query::update()
        .table(USER_TABLE)
        .values(vec![
            (
                UserIden::Disabled,
                SimpleExpr::Value((user.disabled).into()),
            ),
            (
                UserIden::UsedBytes,
                SimpleExpr::Value((user.used_bytes as i64).into()),
            ),
            (
                UserIden::CreatedAt,
                SimpleExpr::Value(user.created_at.into()),
            ),
        ])
        .and_where(Expr::col(UserIden::Id).eq(user.id))
        .returning_all()
        .to_owned();

    let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
    let con = executor.get_con().await?;
    let updated_user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(con).await?;
    Ok(updated_user)
}

pub async fn migrate_users<'a>(
    lmdb: LmDB,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    tracing::info!("Migrating users from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut count = 0;
    for record in lmdb.tables.users.iter(&lmdb_txn)? {
        let (public_key, lmdb_user) = record?;
        let public_key: PublicKey = public_key.into();
        let mut sql_user = UserRepository::create(&public_key, executor).await?;
        sql_user.created_at = nano_seconds_to_timestamp(lmdb_user.created_at)
            .expect("Failed to convert nano seconds to timestamp")
            .naive_utc();
        sql_user.disabled = lmdb_user.disabled;
        sql_user.used_bytes = lmdb_user.used_bytes;
        update_user(&sql_user, executor).await?;
        count += 1;
    }
    tracing::info!("Migrated {} users", count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pubky_common::crypto::Keypair;

    use crate::persistence::{lmdb::tables::users::User, sql::SqlDb};

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();
        // User1
        let user1_pubkey = Keypair::random().public_key();
        let user1_created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            * 1_000_000;
        let lmdb_user1 = User {
            created_at: user1_created_at,
            used_bytes: 100,
            disabled: true,
        };
        lmdb.tables
            .users
            .put(&mut wtxn, &user1_pubkey, &lmdb_user1)
            .unwrap();

        // User2
        let user2_pubkey = Keypair::random().public_key();
        let user2_created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            * 1_000_000;
        let lmdb_user2 = User {
            created_at: user2_created_at,
            used_bytes: 200,
            disabled: false,
        };
        lmdb.tables
            .users
            .put(&mut wtxn, &user2_pubkey, &lmdb_user2)
            .unwrap();
        wtxn.commit().unwrap();

        // Migrate
        migrate_users(lmdb.clone(), &mut sql_db.pool().into())
            .await
            .unwrap();

        // Check
        let sql_user1 = UserRepository::get(&user1_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        assert_eq!(sql_user1.disabled, lmdb_user1.disabled);
        assert_eq!(sql_user1.used_bytes, lmdb_user1.used_bytes);

        let sql_user2 = UserRepository::get(&user2_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        assert_eq!(sql_user2.disabled, lmdb_user2.disabled);
        assert_eq!(sql_user2.used_bytes, lmdb_user2.used_bytes);
    }
}
