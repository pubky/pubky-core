//! Repository for grant-based session entities.

use pubky_common::auth::jws::{GrantId, TokenId};
use sea_query::{Expr, Iden, Order, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::persistence::sql::{
    migrations::m20260325_create_grant_sessions::{GrantSessionIden, GRANT_SESSIONS_TABLE},
    UnifiedExecutor,
};

/// Maximum active sessions per grant on a single homeserver (per proposal).
const MAX_SESSIONS_PER_GRANT: i64 = 1;

/// Repository for grant-based session CRUD operations.
pub struct GrantSessionRepository;

impl GrantSessionRepository {
    /// Create a new session, enforcing the max-1-per-grant limit.
    ///
    /// If the grant already has 1 sessions, the oldest is evicted before
    /// inserting the new one.
    pub async fn create<'a>(
        session: &NewGrantSession,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        Self::enforce_session_limit(&session.grant_id, executor).await?;

        let statement = Query::insert()
            .into_table(GRANT_SESSIONS_TABLE)
            .columns([
                GrantSessionIden::TokenId,
                GrantSessionIden::GrantId,
                GrantSessionIden::ExpiresAt,
            ])
            .values(vec![
                SimpleExpr::Value(session.token_id.to_string().into()),
                SimpleExpr::Value(session.grant_id.to_string().into()),
                SimpleExpr::Value((session.expires_at as i64).into()),
            ])
            .expect("Failed to build insert statement")
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }

    /// Get a session by its token_id.
    pub async fn get_by_token_id<'a>(
        token_id: &TokenId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<GrantSessionEntity, sqlx::Error> {
        let statement = Query::select()
            .from(GRANT_SESSIONS_TABLE)
            .columns([
                GrantSessionIden::Id,
                GrantSessionIden::TokenId,
                GrantSessionIden::GrantId,
                GrantSessionIden::ExpiresAt,
                GrantSessionIden::CreatedAt,
            ])
            .and_where(Expr::col(GrantSessionIden::TokenId).eq(token_id.to_string()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_as_with(&query, values).fetch_one(con).await
    }

    /// Delete all sessions for a given grant (used on revocation).
    pub async fn delete_all_for_grant<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let statement = Query::delete()
            .from_table(GRANT_SESSIONS_TABLE)
            .and_where(Expr::col(GrantSessionIden::GrantId).eq(grant_id.to_string()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }

    /// Enforce the max sessions per grant limit by evicting the oldest if needed.
    async fn enforce_session_limit<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let count = Self::count_for_grant(grant_id, executor).await?;
        if count >= MAX_SESSIONS_PER_GRANT {
            Self::delete_oldest_for_grant(grant_id, executor).await?;
        }
        Ok(())
    }

    async fn count_for_grant<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<i64, sqlx::Error> {
        let statement = Query::select()
            .from(GRANT_SESSIONS_TABLE)
            .expr(Expr::col(GrantSessionIden::Id).count())
            .and_where(Expr::col(GrantSessionIden::GrantId).eq(grant_id.to_string()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        row.try_get::<i64, _>(0)
    }

    async fn delete_oldest_for_grant<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        // Delete the session with the smallest id (oldest) for this grant
        let subquery = Query::select()
            .from(GRANT_SESSIONS_TABLE)
            .column(GrantSessionIden::Id)
            .and_where(Expr::col(GrantSessionIden::GrantId).eq(grant_id.to_string()))
            .order_by(GrantSessionIden::Id, Order::Asc)
            .limit(1)
            .to_owned();

        let (sub_sql, sub_values) = subquery.build_sqlx(PostgresQueryBuilder);

        // Use raw SQL for the DELETE with subquery since sea-query doesn't support DELETE ... WHERE id IN (subquery) easily
        let sql = format!(
            "DELETE FROM {} WHERE {} = ({})",
            GRANT_SESSIONS_TABLE,
            GrantSessionIden::Id.to_string(),
            sub_sql
        );

        let con = executor.get_con().await?;
        sqlx::query_with(&sql, sub_values).execute(con).await?;
        Ok(())
    }
}

/// Data needed to create a new grant session.
pub struct NewGrantSession {
    pub token_id: TokenId,
    pub grant_id: GrantId,
    pub expires_at: u64,
}

/// A grant session entity as stored in the database.
#[derive(Debug, Clone)]
pub struct GrantSessionEntity {
    pub id: i32,
    pub token_id: TokenId,
    pub grant_id: GrantId,
    pub expires_at: i64,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for GrantSessionEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let id: i32 = row.try_get(GrantSessionIden::Id.to_string().as_str())?;
        let token_id: String = row.try_get(GrantSessionIden::TokenId.to_string().as_str())?;
        let token_id =
            TokenId::parse(&token_id).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let grant_id: String = row.try_get(GrantSessionIden::GrantId.to_string().as_str())?;
        let grant_id =
            GrantId::parse(&grant_id).map_err(|e| sqlx::Error::Decode(e.into()))?;
        let expires_at: i64 = row.try_get(GrantSessionIden::ExpiresAt.to_string().as_str())?;
        let created_at = row.try_get(GrantSessionIden::CreatedAt.to_string().as_str())?;

        Ok(GrantSessionEntity {
            id,
            token_id,
            grant_id,
            expires_at,
            created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::{
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
        auth::jws::{ClientId, GrantId, TokenId},
    };

    use crate::persistence::sql::{
        entities::{
            grant::{GrantRepository, NewGrant},
            user::UserRepository,
        },
        SqlDb,
    };

    async fn setup_user_and_grant(db: &SqlDb) -> GrantId {
        let pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let now = chrono::Utc::now().timestamp() as u64;
        let grant_id = GrantId::generate();
        let new_grant = NewGrant {
            grant_id: grant_id.clone(),
            user_id: user.id,
            client_id: ClientId::new("test.app").unwrap(),
            client_cnf_key: Keypair::random().public_key().z32(),
            capabilities: Capabilities::builder().cap(Capability::root()).finish(),
            issued_at: now,
            expires_at: now + 3600,
        };
        GrantRepository::create(&new_grant, &mut db.pool().into())
            .await
            .unwrap();
        grant_id
    }

    fn make_new_session(grant_id: &GrantId) -> NewGrantSession {
        let now = chrono::Utc::now().timestamp() as u64;
        NewGrantSession {
            token_id: TokenId::generate(),
            grant_id: grant_id.clone(),
            expires_at: now + 3600,
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_and_get_session() {
        let db = SqlDb::test().await;
        let grant_id = setup_user_and_grant(&db).await;

        let new_session = make_new_session(&grant_id);
        let token_id = new_session.token_id.clone();
        let expires_at = new_session.expires_at;

        GrantSessionRepository::create(&new_session, &mut db.pool().into())
            .await
            .unwrap();

        let entity = GrantSessionRepository::get_by_token_id(&token_id, &mut db.pool().into())
            .await
            .unwrap();

        assert_eq!(entity.token_id, token_id);
        assert_eq!(entity.grant_id, grant_id);
        assert_eq!(entity.expires_at, expires_at as i64);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_session_limit_evicts_oldest() {
        let db = SqlDb::test().await;
        let grant_id = setup_user_and_grant(&db).await;

        let s1 = make_new_session(&grant_id);
        let s1_token = s1.token_id.clone();
        GrantSessionRepository::create(&s1, &mut db.pool().into())
            .await
            .unwrap();

        let s2 = make_new_session(&grant_id);
        let s2_token = s2.token_id.clone();
        GrantSessionRepository::create(&s2, &mut db.pool().into())
            .await
            .unwrap();

        // Third session should evict the oldest (s1)
        let s3 = make_new_session(&grant_id);
        let s3_token = s3.token_id.clone();
        GrantSessionRepository::create(&s3, &mut db.pool().into())
            .await
            .unwrap();

        // s1 should be evicted
        let result =
            GrantSessionRepository::get_by_token_id(&s1_token, &mut db.pool().into()).await;
        assert!(result.is_err());

        // s2 and s3 should still exist
        GrantSessionRepository::get_by_token_id(&s2_token, &mut db.pool().into())
            .await
            .unwrap();
        GrantSessionRepository::get_by_token_id(&s3_token, &mut db.pool().into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_session_limit_different_grants_independent() {
        let db = SqlDb::test().await;
        let grant_a = setup_user_and_grant(&db).await;

        // Create a second grant for the same user
        let grant_b_id = GrantId::generate();
        let now = chrono::Utc::now().timestamp() as u64;
        let pubkey = Keypair::random().public_key();
        let user = UserRepository::create(&pubkey, &mut db.pool().into())
            .await
            .unwrap();
        let new_grant_b = NewGrant {
            grant_id: grant_b_id.clone(),
            user_id: user.id,
            client_id: ClientId::new("other.app").unwrap(),
            client_cnf_key: Keypair::random().public_key().z32(),
            capabilities: Capabilities::builder().cap(Capability::root()).finish(),
            issued_at: now,
            expires_at: now + 3600,
        };
        GrantRepository::create(&new_grant_b, &mut db.pool().into())
            .await
            .unwrap();

        // 2 sessions for grant_a
        let sa1 = make_new_session(&grant_a);
        let sa1_token = sa1.token_id.clone();
        GrantSessionRepository::create(&sa1, &mut db.pool().into())
            .await
            .unwrap();
        let sa2 = make_new_session(&grant_a);
        let sa2_token = sa2.token_id.clone();
        GrantSessionRepository::create(&sa2, &mut db.pool().into())
            .await
            .unwrap();

        // 2 sessions for grant_b
        let sb1 = make_new_session(&grant_b_id);
        let sb1_token = sb1.token_id.clone();
        GrantSessionRepository::create(&sb1, &mut db.pool().into())
            .await
            .unwrap();
        let sb2 = make_new_session(&grant_b_id);
        let sb2_token = sb2.token_id.clone();
        GrantSessionRepository::create(&sb2, &mut db.pool().into())
            .await
            .unwrap();

        // All 4 sessions should be retrievable
        GrantSessionRepository::get_by_token_id(&sa1_token, &mut db.pool().into())
            .await
            .unwrap();
        GrantSessionRepository::get_by_token_id(&sa2_token, &mut db.pool().into())
            .await
            .unwrap();
        GrantSessionRepository::get_by_token_id(&sb1_token, &mut db.pool().into())
            .await
            .unwrap();
        GrantSessionRepository::get_by_token_id(&sb2_token, &mut db.pool().into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_delete_all_for_grant() {
        let db = SqlDb::test().await;
        let grant_id = setup_user_and_grant(&db).await;

        let s1 = make_new_session(&grant_id);
        let s1_token = s1.token_id.clone();
        GrantSessionRepository::create(&s1, &mut db.pool().into())
            .await
            .unwrap();

        let s2 = make_new_session(&grant_id);
        let s2_token = s2.token_id.clone();
        GrantSessionRepository::create(&s2, &mut db.pool().into())
            .await
            .unwrap();

        GrantSessionRepository::delete_all_for_grant(&grant_id, &mut db.pool().into())
            .await
            .unwrap();

        assert!(
            GrantSessionRepository::get_by_token_id(&s1_token, &mut db.pool().into())
                .await
                .is_err()
        );
        assert!(
            GrantSessionRepository::get_by_token_id(&s2_token, &mut db.pool().into())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_get_nonexistent_session() {
        let db = SqlDb::test().await;
        let result = GrantSessionRepository::get_by_token_id(
            &TokenId::generate(),
            &mut db.pool().into(),
        )
        .await;
        assert!(result.is_err());
    }
}
