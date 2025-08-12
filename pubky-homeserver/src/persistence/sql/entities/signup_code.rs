use std::{fmt::Display, str::FromStr};

use pkarr::PublicKey;
use pubky_common::crypto::random_bytes;
use sea_query::{Expr, Iden, Query, SimpleExpr};
use sqlx::{postgres::PgRow, Executor, FromRow, Row};
use base32::{decode, encode, Alphabet};

use crate::persistence::sql::db_connection::DbConnection;

pub const SIGNUP_CODE_TABLE: &str = "signup_codes";

/// Repository that handles all the queries regarding the UserEntity.
pub struct SignupCodeRepository<'a> {
    pub db: &'a DbConnection,
}

impl<'a> SignupCodeRepository<'a> {

    /// Create a new repository. This is very lightweight.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Create a new user.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'c, E>(&self, id: &SignupCodeId, executor: E) -> Result<SignupCodeEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement =
        Query::insert().into_table(SIGNUP_CODE_TABLE)
            .columns([SignupCodeIden::Id])
            .values(vec![
                SimpleExpr::Value(id.to_string().into()),
            ]).unwrap().returning_all().to_owned();

        let (query, values) = self.db.build_query(statement);

        let code: SignupCodeEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(code)
    }

    /// Get a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get<'c, E>(&self, id: &SignupCodeId, executor: E) -> Result<SignupCodeEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::select().from(SIGNUP_CODE_TABLE)
        .columns([SignupCodeIden::Id, SignupCodeIden::CreatedAt, SignupCodeIden::UsedBy])
        .and_where(Expr::col(SignupCodeIden::Id).eq(id.to_string()))
        .to_owned();
        let (query, values) = self.db.build_query(statement);
        let code: SignupCodeEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(code)
    }

    pub async fn mark_as_used<'c, E>(&self, id: &SignupCodeId, used_by: &PublicKey, executor: E) -> Result<SignupCodeEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::update()
            .table(SIGNUP_CODE_TABLE)
            .values(vec![
                (SignupCodeIden::UsedBy, SimpleExpr::Value(used_by.to_string().into())),
            ])
            .and_where(Expr::col(SignupCodeIden::Id).eq(id.to_string()))
            .returning_all()
            .to_owned();
        
        let (query, values) = self.db.build_query(statement);
        let updated_code: SignupCodeEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SignupCodeEntity {
    pub id: SignupCodeId,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
    pub used_by: Option<PublicKey>,
}

impl FromRow<'_, PgRow> for SignupCodeEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let token: String = row.try_get(SignupCodeIden::Id.to_string().as_str())?;
        let id = SignupCodeId::new(token)
        .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(SignupCodeIden::CreatedAt.to_string().as_str())?;
        let used_by_raw: Option<String> =
            row.try_get(SignupCodeIden::UsedBy.to_string().as_str())?;
        let used_by = used_by_raw
            .map(|s| PublicKey::try_from(s.as_str()).map_err(|e| sqlx::Error::Decode(Box::new(e))))
            .transpose()?;
        Ok(SignupCodeEntity {
            id,
            created_at,
            used_by,
        })
    }
}


#[cfg(test)]
mod tests {
    use pkarr::Keypair;

    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_create_get_signup_code() {
        let db = DbConnection::test().await;
        let signup_code_repo = SignupCodeRepository::new(&db);
        let signup_code_id = SignupCodeId::random();

        // Test create code
        let code = signup_code_repo.create(&signup_code_id, db.pool()).await.unwrap();
        assert_eq!(code.id, signup_code_id);
        assert_eq!(code.used_by, None);

        // Test get code
        let code = signup_code_repo.get(&signup_code_id, db.pool()).await.unwrap();
        assert_eq!(code.id, signup_code_id);
        assert_eq!(code.used_by, None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_mark_as_used() {
        let db = DbConnection::test().await;
        let signup_code_repo = SignupCodeRepository::new(&db);
        let signup_code_id = SignupCodeId::random();
        let _ = signup_code_repo.create(&signup_code_id, db.pool()).await.unwrap();
        
        let user_pubkey = Keypair::random().public_key();

        signup_code_repo.mark_as_used(&signup_code_id, &user_pubkey, db.pool()).await.unwrap();
        let updated_code = signup_code_repo.get(&signup_code_id, db.pool()).await.unwrap();
        assert_eq!(updated_code.id, signup_code_id);
        assert_eq!(updated_code.used_by, Some(user_pubkey));
    }

}