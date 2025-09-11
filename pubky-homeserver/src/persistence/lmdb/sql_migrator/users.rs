use sqlx::types::chrono::DateTime;

use crate::persistence::{
    lmdb::LmDB,
    sql::{user::UserRepository, UnifiedExecutor},
};

pub async fn migrate_users<'a>(
    lmdb: LmDB,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    tracing::info!("Migrating users from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut count = 0;
    for record in lmdb.tables.users.iter(&lmdb_txn)? {
        let (public_key, lmdb_user) = record?;
        let mut sql_user = UserRepository::create(&public_key, executor).await?;
        sql_user.created_at = DateTime::from_timestamp(lmdb_user.created_at as i64, 0)
            .unwrap()
            .naive_utc();
        sql_user.disabled = lmdb_user.disabled;
        sql_user.used_bytes = lmdb_user.used_bytes;
        UserRepository::update(&sql_user, executor).await?;
        count += 1;
    }
    tracing::info!("Migrated {} users", count);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use pkarr::Keypair;

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
        let mut lmdb_user1 = User::default();
        lmdb_user1.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        lmdb_user1.used_bytes = 100;
        lmdb_user1.disabled = true;
        lmdb.tables
            .users
            .put(&mut wtxn, &user1_pubkey, &lmdb_user1)
            .unwrap();

        // User2
        let user2_pubkey = Keypair::random().public_key();
        let mut lmdb_user2 = User::default();
        lmdb_user2.created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        lmdb_user2.used_bytes = 200;
        lmdb_user2.disabled = false;
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
        assert_eq!(
            sql_user1.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            DateTime::from_timestamp(lmdb_user1.created_at as i64, 0)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(sql_user1.disabled, lmdb_user1.disabled);
        assert_eq!(sql_user1.used_bytes, lmdb_user1.used_bytes);

        let sql_user2 = UserRepository::get(&user2_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            sql_user2.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
            DateTime::from_timestamp(lmdb_user2.created_at as i64, 0)
                .unwrap()
                .naive_utc()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        );
        assert_eq!(sql_user2.disabled, lmdb_user2.disabled);
        assert_eq!(sql_user2.used_bytes, lmdb_user2.used_bytes);
    }
}
