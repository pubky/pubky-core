use std::{fmt::Display, str::FromStr};

use base32::{decode, encode, Alphabet};
use pubky_common::crypto::random_bytes;
use pubky_common::crypto::PublicKey;
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::data_directory::user_limit_config::UserLimitConfig;
use crate::persistence::sql::UnifiedExecutor;

pub const SIGNUP_CODE_TABLE: &str = "signup_codes";

/// Repository that handles all the queries regarding the SignupCodeEntity.
pub struct SignupCodeRepository;

impl SignupCodeRepository {
    /// Create a new signup code, optionally with custom limits for users who redeem it.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'a>(
        id: &SignupCodeId,
        custom_limits: Option<&UserLimitConfig>,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<SignupCodeEntity, sqlx::Error> {
        let statement = Query::insert()
            .into_table(SIGNUP_CODE_TABLE)
            .columns([
                SignupCodeIden::Id,
                SignupCodeIden::LimitStorageQuotaMb,
                SignupCodeIden::LimitMaxSessions,
                SignupCodeIden::LimitRateRead,
                SignupCodeIden::LimitRateWrite,
            ])
            .values(vec![
                SimpleExpr::Value(id.to_string().into()),
                SimpleExpr::Value(
                    custom_limits
                        .and_then(|c| c.storage_quota_mb.map(|v| v as i64))
                        .into(),
                ),
                SimpleExpr::Value(
                    custom_limits
                        .and_then(|c| c.max_sessions.map(|v| v as i32))
                        .into(),
                ),
                SimpleExpr::Value(
                    custom_limits
                        .and_then(|c| c.rate_read.clone())
                        .into(),
                ),
                SimpleExpr::Value(
                    custom_limits
                        .and_then(|c| c.rate_write.clone())
                        .into(),
                ),
            ])
            .unwrap()
            .returning_all()
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let code: SignupCodeEntity =
            sqlx::query_as_with(&query, values).fetch_one(con).await?;
        Ok(code)
    }

    /// Get a signup code by its ID.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get<'a>(
        id: &SignupCodeId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<SignupCodeEntity, sqlx::Error> {
        let statement = Query::select()
            .from(SIGNUP_CODE_TABLE)
            .columns([
                SignupCodeIden::Id,
                SignupCodeIden::CreatedAt,
                SignupCodeIden::UsedBy,
                SignupCodeIden::LimitStorageQuotaMb,
                SignupCodeIden::LimitMaxSessions,
                SignupCodeIden::LimitRateRead,
                SignupCodeIden::LimitRateWrite,
            ])
            .and_where(Expr::col(SignupCodeIden::Id).eq(id.to_string()))
            .to_owned();
        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let code: SignupCodeEntity = sqlx::query_as_with(&query, values).fetch_one(con).await?;
        Ok(code)
    }

    /// Get overview statistics for signup codes.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_overview<'a>(
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<SignupCodeOverview, sqlx::Error> {
        // Query to get total number of signup codes
        let total_statement = Query::select()
            .expr(Expr::col(SignupCodeIden::Id).count())
            .from(SIGNUP_CODE_TABLE)
            .to_owned();

        let (total_query, total_values) = total_statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let total_count: i64 = sqlx::query_scalar_with(&total_query, total_values)
            .fetch_one(con)
            .await?;

        // Query to get number of unused signup codes (where used_by is NULL)
        let unused_statement = Query::select()
            .expr(Expr::col(SignupCodeIden::Id).count())
            .from(SIGNUP_CODE_TABLE)
            .and_where(Expr::col(SignupCodeIden::UsedBy).is_null())
            .to_owned();

        let (unused_query, unused_values) = unused_statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let unused_count: i64 = sqlx::query_scalar_with(&unused_query, unused_values)
            .fetch_one(con)
            .await?;

        Ok(SignupCodeOverview {
            num_signup_codes: total_count as u64,
            num_unused_signup_codes: unused_count as u64,
        })
    }

    pub async fn mark_as_used<'a>(
        id: &SignupCodeId,
        used_by: &PublicKey,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<SignupCodeEntity, sqlx::Error> {
        let statement = Query::update()
            .table(SIGNUP_CODE_TABLE)
            .values(vec![(
                SignupCodeIden::UsedBy,
                SimpleExpr::Value(used_by.z32().into()),
            )])
            .and_where(Expr::col(SignupCodeIden::Id).eq(id.to_string()))
            .returning_all()
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let updated_code: SignupCodeEntity =
            sqlx::query_as_with(&query, values).fetch_one(con).await?;
        Ok(updated_code)
    }
}

/// Iden for the signup code table.
/// Basically a list of columns in the signup code table
#[derive(Iden)]
pub enum SignupCodeIden {
    Id,
    CreatedAt,
    UsedBy,
    LimitStorageQuotaMb,
    LimitMaxSessions,
    LimitRateRead,
    LimitRateWrite,
}

/// Signup code id in the format of "JZY0-D6MY-ZFNG".
/// Base32 encoded with the Crockford alphabet, separated by hyphens.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SignupCodeId(pub String);

impl SignupCodeId {
    /// Create a new signup code id.
    /// Returns an error if the id is invalid.
    pub fn new(id: String) -> anyhow::Result<Self> {
        if !Self::is_valid(&id) {
            return Err(anyhow::anyhow!("Invalid signup code id"));
        }
        Ok(Self(id))
    }

    /// Check if a signup code id is in a valid format.
    pub fn is_valid(value: &str) -> bool {
        if value.len() != 14 {
            return false;
        }

        let without_hyphens = value.replace("-", "");
        decode(Alphabet::Crockford, &without_hyphens).is_some()
    }

    /// Create a random signup code id.
    pub fn random() -> Self {
        let bytes = random_bytes::<7>();
        let encoded = encode(Alphabet::Crockford, &bytes).to_uppercase();
        let mut with_hyphens = String::new();
        for (i, ch) in encoded.chars().enumerate() {
            if i > 0 && i % 4 == 0 {
                with_hyphens.push('-');
            }
            with_hyphens.push(ch);
        }

        SignupCodeId(with_hyphens)
    }
}

impl Display for SignupCodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for SignupCodeId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s.to_string())
    }
}

/// Overview statistics for signup codes
#[derive(Debug, Clone)]
pub struct SignupCodeOverview {
    pub num_signup_codes: u64,
    pub num_unused_signup_codes: u64,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SignupCodeEntity {
    pub id: SignupCodeId,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub used_by: Option<PublicKey>,
    /// Per-user storage quota in MB. `None` = use defaults / unlimited.
    pub limit_storage_quota_mb: Option<i64>,
    /// Per-user max sessions. `None` = use defaults / unlimited.
    pub limit_max_sessions: Option<i32>,
    /// Per-user read rate limit. `None` = use defaults / unlimited.
    pub limit_rate_read: Option<String>,
    /// Per-user write rate limit. `None` = use defaults / unlimited.
    pub limit_rate_write: Option<String>,
}

impl SignupCodeEntity {
    /// Extract custom limits, returning `None` if all limit columns are NULL.
    pub fn custom_limits(&self) -> Option<UserLimitConfig> {
        UserLimitConfig::from_nullable_columns(
            self.limit_storage_quota_mb,
            self.limit_max_sessions,
            self.limit_rate_read.clone(),
            self.limit_rate_write.clone(),
        )
    }
}

impl FromRow<'_, PgRow> for SignupCodeEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let token: String = row.try_get(SignupCodeIden::Id.to_string().as_str())?;
        let id = SignupCodeId::new(token).map_err(|e| {
            sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                e,
            )))
        })?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(SignupCodeIden::CreatedAt.to_string().as_str())?;
        let used_by_raw: Option<String> =
            row.try_get(SignupCodeIden::UsedBy.to_string().as_str())?;
        let used_by = used_by_raw
            .map(|s| {
                PublicKey::try_from_z32(s.as_str()).map_err(|e| sqlx::Error::Decode(Box::new(e)))
            })
            .transpose()?;

        let limit_storage_quota_mb: Option<i64> =
            row.try_get(SignupCodeIden::LimitStorageQuotaMb.to_string().as_str())?;
        let limit_max_sessions: Option<i32> =
            row.try_get(SignupCodeIden::LimitMaxSessions.to_string().as_str())?;
        let limit_rate_read: Option<String> =
            row.try_get(SignupCodeIden::LimitRateRead.to_string().as_str())?;
        let limit_rate_write: Option<String> =
            row.try_get(SignupCodeIden::LimitRateWrite.to_string().as_str())?;

        Ok(SignupCodeEntity {
            id,
            created_at,
            used_by,
            limit_storage_quota_mb,
            limit_max_sessions,
            limit_rate_read,
            limit_rate_write,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::persistence::sql::SqlDb;
    use pubky_common::crypto::Keypair;

    use super::*;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_get_signup_code() {
        let db = SqlDb::test().await;
        let signup_code_id = SignupCodeId::random();

        // Test create code without custom limits
        let code = SignupCodeRepository::create(&signup_code_id, None, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(code.id, signup_code_id);
        assert_eq!(code.used_by, None);
        assert_eq!(code.custom_limits(), None);

        // Test get code
        let code = SignupCodeRepository::get(&signup_code_id, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(code.id, signup_code_id);
        assert_eq!(code.used_by, None);
        assert_eq!(code.custom_limits(), None);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_with_custom_limits() {
        let db = SqlDb::test().await;
        let signup_code_id = SignupCodeId::random();

        let config = UserLimitConfig {
            storage_quota_mb: Some(500),
            max_sessions: Some(10),
            rate_read: Some("100r/m".to_string()),
            rate_write: None,
        };

        let code = SignupCodeRepository::create(
            &signup_code_id,
            Some(&config),
            &mut db.pool().into(),
        )
        .await
        .unwrap();
        assert_eq!(code.custom_limits(), Some(config.clone()));

        // Verify get also returns the config
        let code = SignupCodeRepository::get(&signup_code_id, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(code.custom_limits(), Some(config));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_mark_as_used() {
        let db = SqlDb::test().await;
        let signup_code_id = SignupCodeId::random();
        let _ = SignupCodeRepository::create(&signup_code_id, None, &mut db.pool().into())
            .await
            .unwrap();

        let user_pubkey = Keypair::random().public_key();

        SignupCodeRepository::mark_as_used(&signup_code_id, &user_pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let updated_code = SignupCodeRepository::get(&signup_code_id, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(updated_code.id, signup_code_id);
        assert_eq!(updated_code.used_by, Some(user_pubkey));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_get_overview() {
        let db = SqlDb::test().await;

        // Initially, there should be no signup codes
        let overview = SignupCodeRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.num_signup_codes, 0);
        assert_eq!(overview.num_unused_signup_codes, 0);

        // Create some signup codes
        let code1 = SignupCodeId::random();
        let code2 = SignupCodeId::random();
        let code3 = SignupCodeId::random();

        let _ = SignupCodeRepository::create(&code1, None, &mut db.pool().into())
            .await
            .unwrap();
        let _ = SignupCodeRepository::create(&code2, None, &mut db.pool().into())
            .await
            .unwrap();
        let _ = SignupCodeRepository::create(&code3, None, &mut db.pool().into())
            .await
            .unwrap();

        // After creating 3 codes, all should be unused
        let overview = SignupCodeRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.num_signup_codes, 3);
        assert_eq!(overview.num_unused_signup_codes, 3);

        // Mark one code as used
        let user_pubkey = Keypair::random().public_key();
        SignupCodeRepository::mark_as_used(&code1, &user_pubkey, &mut db.pool().into())
            .await
            .unwrap();

        // Now there should be 3 total codes, 2 unused
        let overview = SignupCodeRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.num_signup_codes, 3);
        assert_eq!(overview.num_unused_signup_codes, 2);

        // Mark another code as used
        let user_pubkey2 = Keypair::random().public_key();
        SignupCodeRepository::mark_as_used(&code2, &user_pubkey2, &mut db.pool().into())
            .await
            .unwrap();

        // Now there should be 3 total codes, 1 unused
        let overview = SignupCodeRepository::get_overview(&mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(overview.num_signup_codes, 3);
        assert_eq!(overview.num_unused_signup_codes, 1);
    }
}
