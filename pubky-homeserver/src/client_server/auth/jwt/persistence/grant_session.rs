//! Repository for grant-based session entities.

use pubky_common::auth::jws::{GrantId, TokenId};
use sea_query::{Alias, CommonTableExpression, Expr, Iden, PostgresQueryBuilder, Query, WithClause, WithQuery};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::persistence::sql::{
    migrations::m20260325_create_grant_sessions::{GrantSessionIden, GRANT_SESSIONS_TABLE},
    UnifiedExecutor,
};

/// Repository for grant-based session CRUD operations.
pub struct GrantSessionRepository;

impl GrantSessionRepository {
    /// Create a new session, atomically evicting all previous sessions for the grant.
    ///
    /// Uses a CTE to DELETE + INSERT in a single statement, preventing race
    /// conditions where concurrent requests could temporarily exceed the
    /// one-session-per-grant limit.
    pub async fn create<'a>(
        session: &NewGrantSession,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let delete_cte = CommonTableExpression::new()
            .query(
                Query::delete()
                    .from_table(GRANT_SESSIONS_TABLE)
                    .and_where(
                        Expr::col(GrantSessionIden::GrantId).eq(session.grant_id.to_string()),
                    )
                    .to_owned(),
            )
            .table_name(Alias::new("delete_old"))
            .to_owned();

        let insert = Query::insert()
            .into_table(GRANT_SESSIONS_TABLE)
            .columns([
                GrantSessionIden::TokenId,
                GrantSessionIden::GrantId,
                GrantSessionIden::ExpiresAt,
            ])
            .values_panic([
                session.token_id.to_string().into(),
                session.grant_id.to_string().into(),
                (session.expires_at as i64).into(),
            ])
            .to_owned();

        let statement = WithQuery::new()
            .with_clause(WithClause::new().cte(delete_cte).to_owned())
            .query(insert)
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

    use crate::client_server::auth::jwt::persistence::grant::{GrantRepository, NewGrant};
    use crate::persistence::sql::{entities::user::UserRepository, SqlDb};

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

        // With MAX_SESSIONS_PER_GRANT = 1, each new session evicts the previous one.
        let s1 = make_new_session(&grant_id);
        let s1_token = s1.token_id.clone();
        GrantSessionRepository::create(&s1, &mut db.pool().into())
            .await
            .unwrap();

        // s2 evicts s1
        let s2 = make_new_session(&grant_id);
        let s2_token = s2.token_id.clone();
        GrantSessionRepository::create(&s2, &mut db.pool().into())
            .await
            .unwrap();

        let result =
            GrantSessionRepository::get_by_token_id(&s1_token, &mut db.pool().into()).await;
        assert!(result.is_err(), "s1 should have been evicted by s2");

        // s3 evicts s2
        let s3 = make_new_session(&grant_id);
        let s3_token = s3.token_id.clone();
        GrantSessionRepository::create(&s3, &mut db.pool().into())
            .await
            .unwrap();

        let result =
            GrantSessionRepository::get_by_token_id(&s2_token, &mut db.pool().into()).await;
        assert!(result.is_err(), "s2 should have been evicted by s3");

        // Only s3 survives
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

        // With MAX_SESSIONS_PER_GRANT = 1, each grant independently holds 1 session.
        // sa2 evicts sa1, sb2 evicts sb1.
        let sa1 = make_new_session(&grant_a);
        GrantSessionRepository::create(&sa1, &mut db.pool().into())
            .await
            .unwrap();
        let sa2 = make_new_session(&grant_a);
        let sa2_token = sa2.token_id.clone();
        GrantSessionRepository::create(&sa2, &mut db.pool().into())
            .await
            .unwrap();

        let sb1 = make_new_session(&grant_b_id);
        GrantSessionRepository::create(&sb1, &mut db.pool().into())
            .await
            .unwrap();
        let sb2 = make_new_session(&grant_b_id);
        let sb2_token = sb2.token_id.clone();
        GrantSessionRepository::create(&sb2, &mut db.pool().into())
            .await
            .unwrap();

        // Only the latest session per grant survives
        GrantSessionRepository::get_by_token_id(&sa2_token, &mut db.pool().into())
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
