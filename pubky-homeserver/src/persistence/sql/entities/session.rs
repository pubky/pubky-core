use pubky_common::{capabilities::Capability, crypto::random_bytes};
use sea_query::{Expr, Iden, Query, SimpleExpr};
use sqlx::{postgres::PgRow, Executor, FromRow, Row};

use crate::persistence::sql::{db_connection::DbConnection};

pub const SESSION_TABLE: &str = "sessions";

/// Repository that handles all the queries regarding the UserEntity.
pub struct SessionRepository<'a> {
    pub db: &'a DbConnection,
}

impl<'a> SessionRepository<'a> {

    /// Create a new repository. This is very lightweight.
    pub fn new(db: &'a DbConnection) -> Self {
        Self { db }
    }

    /// Create a new user.
    /// The executor can either be db.pool() or a transaction.
    pub async fn create<'c, E>(&self, user_id: i32, capabilities: &[Capability], executor: E) -> Result<SessionEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let session_secret = base32::encode(base32::Alphabet::Crockford, &random_bytes::<16>());
        let statement =
        Query::insert().into_table(SESSION_TABLE)
            .columns([SessionIden::Secret, SessionIden::Version, SessionIden::User, SessionIden::Capabilities])
            .values(vec![
                SimpleExpr::Value(session_secret.into()),
                SimpleExpr::Value(1.into()),
                SimpleExpr::Value(user_id.into()),
                SimpleExpr::Value(capabilities.iter().map(|c| c.to_string()).collect::<Vec<String>>().into()),
            ]).expect("Failed to build insert statement").returning_all().to_owned();

        let (query, values) = self.db.build_query(statement);

        let session: SessionEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(session)
    }

    /// Get a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    pub async fn get_by_secret<'c, E>(&self, secret: &str, executor: E) -> Result<SessionEntity, sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::select().from(SESSION_TABLE)
        .columns([SessionIden::Id, SessionIden::Secret, SessionIden::Version, SessionIden::User, SessionIden::Capabilities, SessionIden::CreatedAt])
        .and_where(Expr::col(SessionIden::Secret).eq(secret.to_string()))
        .to_owned();
        let (query, values) = self.db.build_query(statement);
        let user: SessionEntity = sqlx::query_as_with(&query, values).fetch_one(executor).await?;
        Ok(user)
    }

    /// Delete a user by their public key.
    /// The executor can either be db.pool() or a transaction.
    pub async fn delete<'c, E>(&self, secret: &str, executor: E) -> Result<(), sqlx::Error>
    where E: Executor<'c, Database = sqlx::Postgres> {
        let statement = Query::delete()
            .from_table(SESSION_TABLE)
            .and_where(Expr::col(SessionIden::Secret).eq(secret.to_string()))
            .to_owned();
        
        let (query, values) = self.db.build_query(statement);
        sqlx::query_with(&query, values).execute(executor).await?;
        Ok(())
    }
}


#[derive(Iden)]
enum SessionIden {
    Id,
    Secret,
    Version,
    User,
    Capabilities,
    CreatedAt,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SessionEntity {
    pub id: i32,
    pub secret: String,
    pub version: u16,
    pub user: i32,
    pub capabilities: Vec<String>,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for SessionEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i32 = row.try_get(SessionIden::Id.to_string().as_str())?;
        let secret: String = row.try_get(SessionIden::Secret.to_string().as_str())?;
        let version: i16 = row.try_get(SessionIden::Version.to_string().as_str())?;
        let user: i32 = row.try_get(SessionIden::User.to_string().as_str())?;
        let capabilities: Vec<String> = row.try_get(SessionIden::Capabilities.to_string().as_str())?;
        let created_at: sqlx::types::chrono::NaiveDateTime =
            row.try_get(SessionIden::CreatedAt.to_string().as_str())?;
        Ok(SessionEntity {
            id,
            secret,
            version: version as u16,
            user,
            capabilities,
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
    async fn test_create_get_session() {
        let db = DbConnection::test().await;
        let user_repo = UserRepository::new(&db);
        let session_repo = SessionRepository::new(&db);
        let user_pubkey = Keypair::random().public_key();

        // Test create user
        let user = user_repo.create(&user_pubkey, db.pool()).await.unwrap();

        // Test create session
        let session = session_repo.create(user.id, &[Capability::root()], db.pool()).await.unwrap();

        // Test get session
        let session = session_repo.get_by_secret(&session.secret, db.pool()).await.unwrap();
        assert_eq!(session.user, user.id);
        assert_eq!(session.capabilities, vec![Capability::root().to_string()]);

        // Test delete session
        session_repo.delete(&session.secret, db.pool()).await.unwrap();

        // Test get session again
        let result = session_repo.get_by_secret(&session.secret, db.pool()).await;
        assert!(result.is_err());
    }

}