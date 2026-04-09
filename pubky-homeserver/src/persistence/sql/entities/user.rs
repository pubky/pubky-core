use pubky_common::crypto::PublicKey;
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::data_directory::user_quota::UserQuota;
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
            .values(vec![SimpleExpr::Value(public_key.z32().into())])
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
                UserIden::QuotaStorageMb,
                UserIden::QuotaMaxSessions,
                UserIden::QuotaRateRead,
                UserIden::QuotaRateWrite,
            ])
            .and_where(Expr::col(UserIden::PublicKey).eq(public_key.z32()))
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
            .and_where(Expr::col(UserIden::PublicKey).eq(public_key.z32()))
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
                UserIden::QuotaStorageMb,
                UserIden::QuotaMaxSessions,
                UserIden::QuotaRateRead,
                UserIden::QuotaRateWrite,
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

    /// Set per-user custom limits. Replaces any existing custom limits entirely.
    ///
    /// Rate limit strings are validated by roundtripping through `BandwidthRate`
    /// parsing to ensure only well-formed values reach the database.
    pub async fn set_quota<'a>(
        user_id: i32,
        config: &UserQuota,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        config
            .validate_rate_roundtrips()
            .map_err(sqlx::Error::Protocol)?;

        let statement = Query::update()
            .table(USER_TABLE)
            .values(vec![
                (
                    UserIden::QuotaStorageMb,
                    SimpleExpr::Value(config.storage_quota_mb_i64().into()),
                ),
                (
                    UserIden::QuotaMaxSessions,
                    SimpleExpr::Value(config.max_sessions_i32().into()),
                ),
                (
                    UserIden::QuotaRateRead,
                    SimpleExpr::Value(config.rate_read_str().into()),
                ),
                (
                    UserIden::QuotaRateWrite,
                    SimpleExpr::Value(config.rate_write_str().into()),
                ),
            ])
            .and_where(Expr::col(UserIden::Id).eq(user_id))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
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

#[cfg(test)]
impl UserRepository {
    /// Test helper: create a user with a storage quota in MB.
    pub async fn create_with_quota_mb(
        db: &crate::persistence::sql::SqlDb,
        pubkey: &pubky_common::crypto::PublicKey,
        quota_mb: u64,
    ) -> UserEntity {
        use crate::data_directory::user_quota::QuotaOverride;
        let user = Self::create(pubkey, &mut db.pool().into()).await.unwrap();
        let config = UserQuota {
            storage_quota_mb: QuotaOverride::Value(quota_mb),
            ..Default::default()
        };
        Self::set_quota(user.id, &config, &mut db.pool().into())
            .await
            .unwrap();
        Self::get(pubkey, &mut db.pool().into()).await.unwrap()
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
    QuotaStorageMb,
    QuotaMaxSessions,
    QuotaRateRead,
    QuotaRateWrite,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserEntity {
    pub id: i32,
    pub public_key: PublicKey,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub disabled: bool,
    pub used_bytes: u64,
    /// Per-user storage quota in MB. `None` = Default (resolved from system config at enforcement time).
    pub quota_storage_mb: Option<i64>,
    /// Per-user max sessions. `None` = Default (resolved from system config at enforcement time).
    pub quota_max_sessions: Option<i32>,
    /// Per-user read rate limit. `None` = Default (resolved from system config at enforcement time).
    pub quota_rate_read: Option<String>,
    /// Per-user write rate limit. `None` = Default (resolved from system config at enforcement time).
    pub quota_rate_write: Option<String>,
}

impl UserEntity {
    /// Apply a custom limit config to this in-memory entity.
    pub fn apply_quota(&mut self, config: &UserQuota) {
        self.quota_storage_mb = config.storage_quota_mb_i64();
        self.quota_max_sessions = config.max_sessions_i32();
        self.quota_rate_read = config.rate_read_str();
        self.quota_rate_write = config.rate_write_str();
    }

    /// Build a `UserQuota` directly from the DB columns.
    /// Integer columns: NULL → Default, -1 → Unlimited, positive → Value.
    /// VARCHAR columns: NULL → Default, "unlimited" → Unlimited, value → Value.
    pub fn quota(&self) -> UserQuota {
        UserQuota::from_nullable_columns(
            self.quota_storage_mb,
            self.quota_max_sessions,
            self.quota_rate_read.clone(),
            self.quota_rate_write.clone(),
        )
    }
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
        let public_key = PublicKey::try_from_z32(raw_pubkey.as_str())
            .map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let disabled: bool = row.try_get(UserIden::Disabled.to_string().as_str())?;
        let raw_used_bytes: i64 = row.try_get(UserIden::UsedBytes.to_string().as_str())?;
        let used_bytes = raw_used_bytes as u64;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(UserIden::CreatedAt.to_string().as_str())?;
        let quota_storage_mb: Option<i64> =
            row.try_get(UserIden::QuotaStorageMb.to_string().as_str())?;
        let quota_max_sessions: Option<i32> =
            row.try_get(UserIden::QuotaMaxSessions.to_string().as_str())?;
        let quota_rate_read: Option<String> =
            row.try_get(UserIden::QuotaRateRead.to_string().as_str())?;
        let quota_rate_write: Option<String> =
            row.try_get(UserIden::QuotaRateWrite.to_string().as_str())?;
        Ok(UserEntity {
            id,
            public_key,
            created_at,
            disabled,
            used_bytes,
            quota_storage_mb,
            quota_max_sessions,
            quota_rate_read,
            quota_rate_write,
        })
    }
}

#[cfg(test)]
mod tests {
    use pubky_common::crypto::Keypair;

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
        assert_eq!(created_user.quota(), UserQuota::default());

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

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_set_quota() {
        use crate::data_directory::quota_config::BandwidthRate;
        use crate::data_directory::user_quota::QuotaOverride;
        use std::str::FromStr;

        let db = SqlDb::test().await;
        let user_pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Initially all limits are default
        assert_eq!(user.quota(), UserQuota::default());

        // Set custom limits
        let config = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            max_sessions: QuotaOverride::Value(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Value(BandwidthRate::from_str("50mb/s").unwrap()),
        };
        UserRepository::set_quota(user.id, &config, &mut db.pool().into())
            .await
            .unwrap();

        // Verify limits are persisted
        let user = UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.quota(), config);

        // Overwrite with all-default via set_quota
        UserRepository::set_quota(user.id, &UserQuota::default(), &mut db.pool().into())
            .await
            .unwrap();

        let user = UserRepository::get(&user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(user.quota(), UserQuota::default());
    }

    #[test]
    fn test_limits_all_null_returns_all_default() {
        let user = UserEntity {
            id: 1,
            public_key: Keypair::random().public_key(),
            created_at: sqlx::types::chrono::NaiveDateTime::default(),
            disabled: false,
            used_bytes: 0,
            quota_storage_mb: None,
            quota_max_sessions: None,
            quota_rate_read: None,
            quota_rate_write: None,
        };

        let limits = user.quota();
        assert_eq!(limits, UserQuota::default());
        assert!(limits.storage_quota_mb.is_default());
        assert!(limits.max_sessions.is_default());
        assert!(limits.rate_read.is_default());
        assert!(limits.rate_write.is_default());
    }

    #[test]
    fn test_limits_mixed_null_and_values() {
        use crate::data_directory::quota_config::BandwidthRate;
        use crate::data_directory::user_quota::QuotaOverride;
        use std::str::FromStr;

        let user = UserEntity {
            id: 1,
            public_key: Keypair::random().public_key(),
            created_at: sqlx::types::chrono::NaiveDateTime::default(),
            disabled: false,
            used_bytes: 0,
            quota_storage_mb: Some(500),
            quota_max_sessions: None,
            quota_rate_read: Some("100mb/m".to_string()),
            quota_rate_write: None,
        };

        let limits = user.quota();
        assert_eq!(limits.storage_quota_mb, QuotaOverride::Value(500));
        assert!(limits.max_sessions.is_default());
        assert_eq!(
            limits.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
        assert_eq!(limits.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_limits_unlimited_values() {
        use crate::data_directory::user_quota::QuotaOverride;

        let user = UserEntity {
            id: 1,
            public_key: Keypair::random().public_key(),
            created_at: sqlx::types::chrono::NaiveDateTime::default(),
            disabled: false,
            used_bytes: 0,
            quota_storage_mb: Some(-1),
            quota_max_sessions: Some(-1),
            quota_rate_read: Some("unlimited".to_string()),
            quota_rate_write: Some("unlimited".to_string()),
        };

        let limits = user.quota();
        assert_eq!(limits.storage_quota_mb, QuotaOverride::Unlimited);
        assert_eq!(limits.max_sessions, QuotaOverride::Unlimited);
        assert_eq!(limits.rate_read, QuotaOverride::Unlimited);
        assert_eq!(limits.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_limits_invalid_rate_string_treated_as_default() {
        use crate::data_directory::user_quota::QuotaOverride;

        let user = UserEntity {
            id: 1,
            public_key: Keypair::random().public_key(),
            created_at: sqlx::types::chrono::NaiveDateTime::default(),
            disabled: false,
            used_bytes: 0,
            quota_storage_mb: None,
            quota_max_sessions: None,
            quota_rate_read: Some("rubbish".to_string()),
            quota_rate_write: Some("also_rubbish".to_string()),
        };

        let limits = user.quota();
        // Invalid rate strings → Default (with warning logged)
        assert_eq!(limits.rate_read, QuotaOverride::Default);
        assert_eq!(limits.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_apply_quota() {
        use crate::data_directory::quota_config::BandwidthRate;
        use crate::data_directory::user_quota::QuotaOverride;
        use std::str::FromStr;

        let mut user = UserEntity {
            id: 1,
            public_key: Keypair::random().public_key(),
            created_at: sqlx::types::chrono::NaiveDateTime::default(),
            disabled: false,
            used_bytes: 0,
            quota_storage_mb: None,
            quota_max_sessions: None,
            quota_rate_read: None,
            quota_rate_write: None,
        };

        let config = UserQuota {
            storage_quota_mb: QuotaOverride::Value(500),
            max_sessions: QuotaOverride::Value(10),
            rate_read: QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap()),
            rate_write: QuotaOverride::Unlimited,
        };
        user.apply_quota(&config);

        assert_eq!(user.quota_storage_mb, Some(500));
        assert_eq!(user.quota_max_sessions, Some(10));
        assert_eq!(user.quota_rate_read, Some("100mb/m".to_string()));
        assert_eq!(user.quota_rate_write, Some("unlimited".to_string()));
    }
}
