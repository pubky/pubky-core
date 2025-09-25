use pkarr::PublicKey;
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::persistence::sql::UnifiedExecutor;

pub const USER_TABLE: &str = "users";

/// Repository that handles all the queries regarding the UserEntity.
pub struct UserRepository;

impl UserRepository {
    /// Create a new user.
    pub async fn create<'a>(
        public_key: &PublicKey,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<UserEntity, sqlx::Error> {
        let statement = Query::insert()
            .into_table(USER_TABLE)
            .columns([UserIden::PublicKey])
            .values(vec![SimpleExpr::Value(public_key.to_string().into())])
            .unwrap()
            .returning_all()
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);

        let con = executor.get_con().await?;
        let user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(con).await?;

        Ok(user)
    }

    /// Get a user by their public key.
    pub async fn get<'a>(
        public_key: &PublicKey,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<UserEntity, sqlx::Error> {
        let statement = Query::select()
            .from(USER_TABLE)
            .columns([
                UserIden::Id,
                UserIden::PublicKey,
                UserIden::CreatedAt,
                UserIden::Disabled,
                UserIden::UsedBytes,
            ])
            .and_where(Expr::col(UserIden::PublicKey).eq(public_key.to_string()))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(con).await?;
        Ok(user)
    }

    /// Get the id of a user by their public key.
    pub async fn get_id<'a>(
        public_key: &PublicKey,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<i32, sqlx::Error> {
        let statement = Query::select()
            .from(USER_TABLE)
            .columns([UserIden::Id])
            .and_where(Expr::col(UserIden::PublicKey).eq(public_key.to_string()))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let id: i32 = sqlx::query_with(&query, values)
            .fetch_one(con)
            .await?
            .try_get(UserIden::Id.to_string().as_str())?;
        Ok(id)
    }

    /// Get all users.
    pub async fn get_all<'a>(
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<UserEntity>, sqlx::Error> {
        let statement = Query::select()
            .from(USER_TABLE)
            .columns([
                UserIden::Id,
                UserIden::PublicKey,
                UserIden::CreatedAt,
                UserIden::Disabled,
                UserIden::UsedBytes,
            ])
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let users: Vec<UserEntity> = sqlx::query_as_with(&query, values).fetch_all(con).await?;
        Ok(users)
    }

    /// Get the overview of the users.
    pub async fn get_overview<'a>(
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<UserOverview, sqlx::Error> {
        // Get total count and total used bytes
        let statement = Query::select()
            .from(USER_TABLE)
            .expr_as(Expr::col(UserIden::Id).count(), "count")
            .expr_as(
                Expr::col(UserIden::UsedBytes)
                    .sum()
                    .div(1024 * 1024)
                    .cast_as("bigint"),
                "total_used_mbytes",
            )
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&query, values)
            .fetch_one(executor.get_con().await?)
            .await?;

        let count: i64 = row.try_get("count")?;
        let total_used_bytes: Option<i64> = row.try_get("total_used_mbytes")?;

        // Get disabled count
        let statement = Query::select()
            .from(USER_TABLE)
            .expr_as(Expr::col(UserIden::Id).count(), "disabled_count")
            .and_where(Expr::col(UserIden::Disabled).eq(true))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&query, values)
            .fetch_one(executor.get_con().await?)
            .await?;

        let disabled_count: i64 = row.try_get("disabled_count")?;

        // Create the overview
        let overview = UserOverview {
            count: count as u64,
            disabled_count: disabled_count as u64,
            total_used_mb: total_used_bytes.unwrap_or(0) as u64,
        };

        Ok(overview)
    }

    pub async fn update<'a>(
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
            ])
            .and_where(Expr::col(UserIden::Id).eq(user.id))
            .returning_all()
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let updated_user: UserEntity = sqlx::query_as_with(&query, values).fetch_one(con).await?;
        Ok(updated_user)
    }

    /// Delete a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    #[cfg(test)]
    pub async fn delete<'a>(
        user_id: i32,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let statement = Query::delete()
            .from_table(USER_TABLE)
            .and_where(Expr::col(UserIden::Id).eq(user_id))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserOverview {
    pub count: u64,
    pub disabled_count: u64,
    pub total_used_mb: u64,
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
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(UserIden::CreatedAt.to_string().as_str())?;
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

    use crate::persistence::sql::SqlDb;

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_get_user() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let created_user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(created_user.public_key, user_pubkey);
        assert!(!created_user.disabled);
        assert_eq!(created_user.used_bytes, 0);
        assert_eq!(created_user.id, 1);

        // Test get user
        let user = UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.public_key, user_pubkey);
        assert!(!user.disabled);
        assert_eq!(user.used_bytes, 0);
        assert_eq!(user.id, created_user.id);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_user_twice() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.public_key, user_pubkey);
        assert!(!user.disabled);
        assert_eq!(user.used_bytes, 0);

        UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .expect_err("Should fail to create user twice");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_update_user() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();
        let mut user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        user.used_bytes = 10;
        user.disabled = true;

        UserRepository::update(&user, &mut db.pool().into())
            .await
            .unwrap();
        let updated_user = UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(updated_user.id, user.id);
        assert!(updated_user.disabled);
        assert_eq!(updated_user.used_bytes, 10);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_delete_user() {
        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();

        // Create a user first
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.public_key, user_pubkey);

        // Verify the user exists
        let retrieved_user = UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(retrieved_user.public_key, user_pubkey);

        // Delete the user
        UserRepository::delete(user.id, &mut db.pool().into())
            .await
            .unwrap();

        // Verify the user is deleted
        UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .expect_err("User should be deleted");
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_get_overview() {
        let db = SqlDb::test().await;

        // Initially, there should be no users
        let overview = UserRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.count, 0);
        assert_eq!(overview.disabled_count, 0);
        assert_eq!(overview.total_used_mb, 0);

        // Create multiple users with different states
        let user1_pubkey = Keypair::random().public_key();
        let user2_pubkey = Keypair::random().public_key();
        let user3_pubkey = Keypair::random().public_key();

        let mut user1 = UserRepository::create(&user1_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let mut user2 = UserRepository::create(&user2_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let _ = UserRepository::create(&user3_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Set some user properties
        let megabytes = 1024 * 1024;
        user1.used_bytes = megabytes * 1024;
        user1.disabled = false;
        UserRepository::update(&user1, &mut db.pool().into())
            .await
            .unwrap();

        user2.used_bytes = megabytes * 2048;
        user2.disabled = true;
        UserRepository::update(&user2, &mut db.pool().into())
            .await
            .unwrap();

        // Get overview
        let overview = UserRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.count, 3); // Total users
        assert_eq!(overview.disabled_count, 1); // One disabled user
        assert_eq!(overview.total_used_mb, 3072); // 1024 + 2048
    }
}
