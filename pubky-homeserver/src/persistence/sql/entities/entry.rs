use pkarr::PublicKey;
use sea_query::{Expr, Iden, Query, SimpleExpr};
use sqlx::{postgres::PgRow, Executor, FromRow, Row};

use crate::{
    persistence::sql::{
        db_connection::DbConnection,
        entities::user::{UserIden, USER_TABLE},
    },
    shared::webdav::{EntryPath, WebDavPath},
};

pub const ENTRY_TABLE: &str = "entries";

/// Repository that handles all the queries regarding the EntryEntity.
pub struct EntryRepository<'a> {
    pub db: &'a DbConnection,
}

impl<'a> EntryRepository<'a> {
    /// Create a new repository. This is very lightweight.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Create a new entry.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'c, E>(
        &self,
        user_id: i32,
        path: &WebDavPath,
        content_hash: &[u8; 32],
        content_length: u64,
        content_type: &str,
        executor: E,
    ) -> Result<i64, sqlx::Error>
    where
        E: Executor<'c, Database = sqlx::Postgres>,
    {
        let statement = Query::insert()
            .into_table(ENTRY_TABLE)
            .columns([
                EntryIden::User,
                EntryIden::Path,
                EntryIden::ContentHash,
                EntryIden::ContentLength,
                EntryIden::ContentType,
            ])
            .values(vec![
                SimpleExpr::Value(user_id.into()),
                SimpleExpr::Value(path.as_str().into()),
                SimpleExpr::Value(content_hash.to_vec().into()),
                SimpleExpr::Value(content_length.into()),
                SimpleExpr::Value(content_type.to_string().into()),
            ])
            .expect("Failed to build insert statement")
            .returning_col(EntryIden::Id)
            .to_owned();

        let (query, values) = self.db.build_query(statement);

        let ret_row: PgRow = sqlx::query_with(&query, values).fetch_one(executor).await?;
        let entry_id: i64 = ret_row.try_get(EntryIden::Id.to_string().as_str())?;
        Ok(entry_id)
    }

    /// Get an entry by its id.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get<'c, E>(&self, id: i64, executor: E) -> Result<EntryEntity, sqlx::Error>
    where
        E: Executor<'c, Database = sqlx::Postgres>,
    {
        let statement = Query::select()
            .from(ENTRY_TABLE)
            .columns([
                (ENTRY_TABLE, EntryIden::Id),
                (ENTRY_TABLE, EntryIden::User),
                (ENTRY_TABLE, EntryIden::Path),
                (ENTRY_TABLE, EntryIden::ContentHash),
                (ENTRY_TABLE, EntryIden::ContentLength),
                (ENTRY_TABLE, EntryIden::ContentType),
                (ENTRY_TABLE, EntryIden::CreatedAt),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .left_join(
                USER_TABLE,
                Expr::col((ENTRY_TABLE, EntryIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((ENTRY_TABLE, EntryIden::Id)).eq(id))
            .to_owned();
        let (query, values) = self.db.build_query(statement);
        let entry: EntryEntity = sqlx::query_as_with(&query, values)
            .fetch_one(executor)
            .await?;
        Ok(entry)
    }

    /// Get an entry by its path.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_path<'c, E>(&self, path: &EntryPath, executor: E) -> Result<EntryEntity, sqlx::Error>
    where
        E: Executor<'c, Database = sqlx::Postgres>,
    {
        let statement = Query::select()
            .from(ENTRY_TABLE)
            .columns([
                (ENTRY_TABLE, EntryIden::Id),
                (ENTRY_TABLE, EntryIden::User),
                (ENTRY_TABLE, EntryIden::Path),
                (ENTRY_TABLE, EntryIden::ContentHash),
                (ENTRY_TABLE, EntryIden::ContentLength),
                (ENTRY_TABLE, EntryIden::ContentType),
                (ENTRY_TABLE, EntryIden::CreatedAt),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .left_join(
                USER_TABLE,
                Expr::col((ENTRY_TABLE, EntryIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((ENTRY_TABLE, EntryIden::Path)).eq(path.path().as_str()))
            .and_where(Expr::col((USER_TABLE, UserIden::PublicKey)).eq(path.pubkey().to_string()))
            .to_owned();
        let (query, values) = self.db.build_query(statement);
        let entry: EntryEntity = sqlx::query_as_with(&query, values)
            .fetch_one(executor)
            .await?;
        Ok(entry)
    }



    /// Delete an entry by its id.
    /// The executor can either be db.pool() or a transaction.
    pub async fn delete<'c, E>(&self, id: i64, executor: E) -> Result<(), sqlx::Error>
    where
        E: Executor<'c, Database = sqlx::Postgres>,
    {
        let statement = Query::delete()
            .from_table(ENTRY_TABLE)
            .and_where(Expr::col((ENTRY_TABLE, EntryIden::Id)).eq(id))
            .to_owned();
        let (query, values) = self.db.build_query(statement);
        sqlx::query_with(&query, values).execute(executor).await?;
        Ok(())
    }

    /// Delete an entry by its path.
    /// The executor can either be db.pool() or a transaction.
    pub async fn delete_by_path<'c, E>(&self, path: &EntryPath, executor: E) -> Result<(), sqlx::Error>
    where
        E: Executor<'c, Database = sqlx::Postgres>,
    {
        // First get the id of the entry to delete
        let subquery = Query::select()
            .column((ENTRY_TABLE, EntryIden::Id))
            .from(ENTRY_TABLE)
            .left_join(
                USER_TABLE,
                Expr::col((ENTRY_TABLE, EntryIden::User)).eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((ENTRY_TABLE, EntryIden::Path)).eq(path.path().as_str()))
            .and_where(Expr::col((USER_TABLE, UserIden::PublicKey)).eq(path.pubkey().to_string()))
            .to_owned();

        // Then delete the entry by the id
        let statement = Query::delete()
            .from_table(ENTRY_TABLE)
            .and_where(Expr::col((ENTRY_TABLE, EntryIden::Id)).in_subquery(subquery))
            .to_owned();
        let (query, values) = self.db.build_query(statement);
        sqlx::query_with(&query, values).execute(executor).await?;
        Ok(())
    }
}

#[derive(Iden)]
enum EntryIden {
    Id,
    Path,
    User,
    ContentHash,
    ContentLength,
    ContentType,
    CreatedAt,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct EntryEntity {
    pub id: i64,
    pub user_id: i32,
    pub path: EntryPath,
    pub content_hash: [u8; 32],
    pub content_length: u64,
    pub content_type: String,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for EntryEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.try_get(EntryIden::Id.to_string().as_str())?;
        let user_id: i32 = row.try_get(EntryIden::User.to_string().as_str())?;
        let user_pubkey: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let user_pubkey: PublicKey = user_pubkey.parse().map_err(|e: pkarr::errors::PublicKeyError| sqlx::Error::Decode(e.into()))?;
        let path: String = row.try_get(EntryIden::Path.to_string().as_str())?;
        let webdav_path = WebDavPath::new(&path).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let entry_path = EntryPath::new(user_pubkey, webdav_path);
        let content_hash_vec: Vec<u8> = row.try_get(EntryIden::ContentHash.to_string().as_str())?;
        
        // Ensure content_hash is exactly 32 bytes
        let content_hash: [u8; 32] = content_hash_vec
            .try_into()
            .map_err(|_| sqlx::Error::Decode("Content hash must be exactly 32 bytes".into()))?;
            
        let content_length: i64 = row.try_get(EntryIden::ContentLength.to_string().as_str())?;
        let content_type: String = row.try_get(EntryIden::ContentType.to_string().as_str())?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(EntryIden::CreatedAt.to_string().as_str())?;
        Ok(EntryEntity {
            id,
            user_id,
            path: entry_path,
            content_hash,
            content_length: content_length as u64,
            content_type,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use crate::persistence::sql::entities::user::UserRepository;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_get_entry() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let entry_repo = EntryRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();

        // Test create entry
        let entry_id = entry_repo
            .create(
                user.id,
                &WebDavPath::new("/test").unwrap(),
                &[0; 32],
                100,
                "text/plain",
                db.pool(),
            )
            .await
            .unwrap();

        // Test get entry
        let entry = entry_repo.get(entry_id, db.pool()).await.unwrap();
        assert_eq!(entry.id, entry_id);
        assert_eq!(entry.user_id, user.id);
        assert_eq!(entry.path, EntryPath::new(user_pubkey.clone(), WebDavPath::new("/test").unwrap()));
        assert_eq!(entry.content_hash, [0; 32]);
        assert_eq!(entry.content_length, 100);
        assert_eq!(entry.content_type, "text/plain");

        // test get by path
        let entry_by_path = entry_repo.get_by_path(&entry.path, db.pool()).await.unwrap();
        assert_eq!(entry_by_path.id, entry_id);

        // test delete
        entry_repo.delete_by_path(&entry.path, db.pool()).await.unwrap();
        entry_repo.get_by_path(&entry.path, db.pool()).await.expect_err("Entry should be deleted");

    }
}
