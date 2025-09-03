use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;

use crate::{
    persistence::{
        lmdb::{tables::entries::Entry, LmDB},
        sql::{
            entry::{EntryIden, ENTRY_TABLE},
            user::UserRepository,
            UnifiedExecutor,
        },
    },
    shared::{timestamp_to_sqlx_datetime, webdav::EntryPath},
};

/// Create a new signup code.
/// The executor can either be db.pool() or a transaction.
pub async fn create<'a>(
    entry_path: &EntryPath,
    entry: &Entry,
    executor: &mut UnifiedExecutor<'a>,
) -> Result<(), sqlx::Error> {
    let sql_user = UserRepository::get(entry_path.pubkey(), executor).await?;
    let created_at = timestamp_to_sqlx_datetime(entry.timestamp());
    let statement = Query::insert()
        .into_table(ENTRY_TABLE)
        .columns([
            EntryIden::User,
            EntryIden::Path,
            EntryIden::ContentHash,
            EntryIden::ContentLength,
            EntryIden::ContentType,
            EntryIden::ModifiedAt,
            EntryIden::CreatedAt,
        ])
        .values(vec![
            SimpleExpr::Value(sql_user.id.into()),
            SimpleExpr::Value(entry_path.path().as_str().into()),
            SimpleExpr::Value(entry.content_hash().as_bytes().to_vec().into()),
            SimpleExpr::Value((entry.content_length() as u64).into()),
            SimpleExpr::Value(entry.content_type().to_string().into()),
            SimpleExpr::Value(created_at.into()),
            SimpleExpr::Value(created_at.into()),
        ])
        .expect("Failed to build insert statement")
        .returning_col(EntryIden::Id)
        .to_owned();

    let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

    let con = executor.get_con().await?;
    sqlx::query_with(&query, values).fetch_one(con).await?;
    Ok(())
}

pub async fn migrate_entries<'a>(
    lmdb: LmDB,
    executor: &mut UnifiedExecutor<'a>,
) -> anyhow::Result<()> {
    tracing::info!("Migrating entries from LMDB to SQL");
    let lmdb_txn = lmdb.env.read_txn()?;
    let mut count = 0;
    for record in lmdb.tables.entries.iter(&lmdb_txn)? {
        let (path, bytes) = record?;
        let entry_path: EntryPath = path.parse()?;
        let entry = Entry::deserialize(bytes)?;
        create(&entry_path, &entry, executor).await?;
        count += 1;
    }
    tracing::info!("Migrated {} entries", count);
    Ok(())
}

#[cfg(test)]
mod tests {

    use pkarr::Keypair;
    use pubky_common::{crypto::Hash, timestamp::Timestamp};

    use crate::{
        persistence::sql::{entry::EntryRepository, SqlDb},
        shared::webdav::WebDavPath,
    };

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrate() {
        let lmdb = LmDB::test();
        let sql_db = SqlDb::test().await;

        let mut wtxn = lmdb.env.write_txn().unwrap();

        // Entry1
        let user1_pubkey = Keypair::random().public_key();
        UserRepository::create(&user1_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        let entry_path1 =
            EntryPath::new(user1_pubkey, WebDavPath::new("/folder1/file1.txt").unwrap());
        let mut entry1 = Entry::new();
        entry1.set_content_hash(Hash::from_bytes([0u8; 32]));
        entry1.set_content_length(100);
        entry1.set_content_type("text/plain".to_string());
        entry1.set_timestamp(&Timestamp::now());
        lmdb.tables
            .entries
            .put(&mut wtxn, entry_path1.as_str(), &entry1.serialize())
            .unwrap();

        // Entry2
        let user2_pubkey = Keypair::random().public_key();
        UserRepository::create(&user2_pubkey, &mut sql_db.pool().into())
            .await
            .unwrap();
        let entry_path2 =
            EntryPath::new(user2_pubkey, WebDavPath::new("/folder2/file2.txt").unwrap());
        let mut entry2 = Entry::new();
        entry2.set_content_hash(Hash::from_bytes([1u8; 32]));
        entry2.set_content_length(200);
        entry2.set_content_type("text/plain".to_string());
        entry2.set_timestamp(&Timestamp::now());
        lmdb.tables
            .entries
            .put(&mut wtxn, entry_path2.as_str(), &entry2.serialize())
            .unwrap();

        wtxn.commit().unwrap();

        // Migrate
        migrate_entries(lmdb.clone(), &mut sql_db.pool().into())
            .await
            .unwrap();

        // Check
        let sql_entry1 = EntryRepository::get_by_path(&entry_path1, &mut sql_db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            sql_entry1.content_hash.to_hex(),
            entry1.content_hash().to_hex()
        );
        assert_eq!(sql_entry1.content_length, entry1.content_length() as u64);
        assert_eq!(sql_entry1.content_type, entry1.content_type());
        assert_eq!(
            sql_entry1.modified_at.and_utc().timestamp() as u64,
            entry1.timestamp().as_u64() / 1_000_000
        );
        assert_eq!(
            sql_entry1.created_at.and_utc().timestamp() as u64,
            entry1.timestamp().as_u64() / 1_000_000
        );

        let sql_entry2 = EntryRepository::get_by_path(&entry_path2, &mut sql_db.pool().into())
            .await
            .unwrap();
        assert_eq!(
            sql_entry2.content_hash.to_hex(),
            entry2.content_hash().to_hex()
        );
        assert_eq!(sql_entry2.content_length, entry2.content_length() as u64);
        assert_eq!(sql_entry2.content_type, entry2.content_type());
        assert_eq!(
            sql_entry2.modified_at.and_utc().timestamp() as u64,
            entry2.timestamp().as_u64() / 1_000_000
        );
        assert_eq!(
            sql_entry2.created_at.and_utc().timestamp() as u64,
            entry2.timestamp().as_u64() / 1_000_000
        );
    }
}
