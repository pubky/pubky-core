use pkarr::PublicKey;
use sea_query::{Expr, Iden, Query, SimpleExpr};
use sqlx::{postgres::PgRow, Executor, FromRow, Row};

use crate::persistence::sql::db_connection::DbConnection;

pub const USER_TABLE: &str = "users";

/// Repository that handles all the queries regarding the UserEntity.
pub struct UserRepository<'a> {
    pub db: &'a DbConnection,
}

impl<'a> UserRepository<'a> {

    /// Create a new repository. This is very lightweight.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Create a new user.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'c, E>(&self, public_key: &PublicKey, executor: E) -> Result<UserEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> + Clone {
        let statement =
        Query::insert().into_table(USER_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![
                SimpleExpr::Value(public_key.to_string().into()),
            ]).unwrap().returning_all().to_owned();

        let (query, values) = self.db.build_query(statement);

        let user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        

        Ok(user)
    }

    /// Get a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get<'c, E>(&self, public_key: &PublicKey, executor: E) -> Result<UserEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::select().from(USER_TABLE)
        .columns([UserIden::Id, UserIden::PublicKey, UserIden::CreatedAt, UserIden::Disabled, UserIden::UsedBytes])
        .and_where(Expr::col(UserIden::PublicKey).eq(public_key.to_string()))
        .to_owned();
        let (query, values) = self.db.build_query(statement);
        let user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(user)
    }

    pub async fn update<'c, E>(&self, user: &UserEntity, executor: E) -> Result<UserEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::update()
            .table(USER_TABLE)
            .values(vec![
                (UserIden::Disabled, SimpleExpr::Value((user.disabled).into())),
                (UserIden::UsedBytes, SimpleExpr::Value((user.used_bytes as i64).into())),
            ])
            .and_where(Expr::col(UserIden::Id).eq(user.id))
            .returning_all()
            .to_owned();
        
        let (query, values) = self.db.build_query(statement);
        let updated_user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(updated_user)
    }

    /// Delete a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    pub async fn delete<'c, E>(&self, user_id: i32, executor: E) -> Result<(), sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::delete()
            .from_table(USER_TABLE)
            .and_where(Expr::col(UserIden::Id).eq(user_id))
            .to_owned();
        
        let (query, values) = self.db.build_query(statement);
        sqlx::query_with(&query, values).execute(executor).await?;
        Ok(())
    }
}


/// Iden for the user table.
/// Basically a list of columns in the user table
#[derive(Iden)]
pub enum UserIden {
    Id,
    PublicKey,
    CreatedAt,
    Disabled,
    UsedBytes,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserEntity {
    pub id: i32,
    pub public_key: PublicKey,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub disabled: bool,
    pub used_bytes: u64,
}

impl FromRow<'_, PgRow> for UserEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i32 = row.try_get(UserIden::Id.to_string().as_str())?;
        let raw_pubkey: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
        let public_key = PublicKey::try_from(raw_pubkey.as_str())
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let disabled: bool = row.try_get(UserIden::Disabled.to_string().as_str())?;
        let raw_used_bytes: i64 = row.try_get(UserIden::UsedBytes.to_string().as_str())?;
        let used_bytes = raw_used_bytes as u64;
        let created_at: sqlx::types::chrono::NaiveDateTime = row.try_get(UserIden::CreatedAt.to_string().as_str())?;
        Ok(UserEntity {
            id,
            public_key,
            created_at,
            disabled,
            used_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_get_user() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let created_user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(created_user.public_key, user_pubkey);
        assert_eq!(created_user.disabled, false);
        assert_eq!(created_user.used_bytes, 0);
        assert_eq!(created_user.id, 1);

        // Test get user
        let user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(user.public_key, user_pubkey);
        assert_eq!(user.disabled, false);
        assert_eq!(user.used_bytes, 0);
        assert_eq!(user.id, created_user.id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_user_twice() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(user.public_key, user_pubkey);
        assert_eq!(user.disabled, false);
        assert_eq!(user.used_bytes, 0);

        user_repo.create(&user_pubkey, db.pool()).await.expect_err("Should fail to create user twice");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_update_user() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();
        let mut user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
        
        user.used_bytes = 10;
        user.disabled = true;

        user_repo.update(&user, db.pool()).await.unwrap();
        let updated_user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(updated_user.id, user.id);
        assert_eq!(updated_user.disabled, true);
        assert_eq!(updated_user.used_bytes, 10);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_user() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Create a user first
        let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(user.public_key, user_pubkey);

        // Verify the user exists
        let retrieved_user = user_repo.get(&user_pubkey, db.pool()).await.unwrap();
        assert_eq!(retrieved_user.public_key, user_pubkey);

        // Delete the user
        user_repo.delete(user.id, db.pool()).await.unwrap();

        // Verify the user no longer exists
        let result = user_repo.get(&user_pubkey, db.pool()).await;
        assert!(result.is_err());
    }
}