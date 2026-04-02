//! Repository for Grant entities (grant auth).

use pubky_common::{
    capabilities::Capabilities,
    crypto::PublicKey,
    auth::jws::{ClientId, GrantId},
};
use sea_query::{Expr, Iden, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_binder::SqlxBinder;
use sqlx::{postgres::PgRow, FromRow, Row};

use crate::persistence::sql::{
    entities::user::{UserIden, USER_TABLE},
    migrations::m20260325_create_grant_sessions::{GrantIden, GRANTS_TABLE},
    UnifiedExecutor,
};

/// Repository for grant CRUD operations.
pub struct GrantRepository;

impl GrantRepository {
    /// Insert a grant. Ignores if grant_id already exists (idempotent).
    pub async fn create<'a>(
        grant: &NewGrant,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let statement = Query::insert()
            .into_table(GRANTS_TABLE)
            .columns([
                GrantIden::GrantId,
                GrantIden::User,
                GrantIden::ClientId,
                GrantIden::ClientCnfKey,
                GrantIden::Capabilities,
                GrantIden::IssuedAt,
                GrantIden::ExpiresAt,
            ])
            .values(vec![
                SimpleExpr::Value(grant.grant_id.to_string().into()),
                SimpleExpr::Value(grant.user_id.into()),
                SimpleExpr::Value(grant.client_id.to_string().into()),
                SimpleExpr::Value(grant.client_cnf_key.clone().into()),
                SimpleExpr::Value(grant.capabilities.to_string().into()),
                SimpleExpr::Value((grant.issued_at as i64).into()),
                SimpleExpr::Value((grant.expires_at as i64).into()),
            ])
            .expect("invariant: values count matches columns count")
            .on_conflict(
                sea_query::OnConflict::column(GrantIden::GrantId)
                    .do_nothing()
                    .to_owned(),
            )
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }

    /// Get a grant by its grant_id.
    pub async fn get_by_grant_id<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<GrantEntity, sqlx::Error> {
        let statement = Query::select()
            .from(GRANTS_TABLE)
            .columns([
                (GRANTS_TABLE, GrantIden::Id),
                (GRANTS_TABLE, GrantIden::GrantId),
                (GRANTS_TABLE, GrantIden::User),
                (GRANTS_TABLE, GrantIden::ClientId),
                (GRANTS_TABLE, GrantIden::ClientCnfKey),
                (GRANTS_TABLE, GrantIden::Capabilities),
                (GRANTS_TABLE, GrantIden::IssuedAt),
                (GRANTS_TABLE, GrantIden::ExpiresAt),
                (GRANTS_TABLE, GrantIden::RevokedAt),
                (GRANTS_TABLE, GrantIden::CreatedAt),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .left_join(
                USER_TABLE,
                Expr::col((GRANTS_TABLE, GrantIden::User))
                    .eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(
                Expr::col((GRANTS_TABLE, GrantIden::GrantId)).eq(grant_id.to_string()),
            )
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_as_with(&query, values).fetch_one(con).await
    }

    /// Revoke a grant by setting revoked_at to the current unix timestamp.
    pub async fn revoke<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let statement = Query::update()
            .table(GRANTS_TABLE)
            .value(GrantIden::RevokedAt, SimpleExpr::Value(now.into()))
            .and_where(Expr::col(GrantIden::GrantId).eq(grant_id.to_string()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_with(&query, values).execute(con).await?;
        Ok(())
    }

    /// Check if a grant has been revoked.
    pub async fn is_revoked<'a>(
        grant_id: &GrantId,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<bool, sqlx::Error> {
        let statement = Query::select()
            .from(GRANTS_TABLE)
            .column(GrantIden::RevokedAt)
            .and_where(Expr::col(GrantIden::GrantId).eq(grant_id.to_string()))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        let row: PgRow = sqlx::query_with(&query, values).fetch_one(con).await?;
        let revoked_at: Option<i64> = row.try_get(GrantIden::RevokedAt.to_string().as_str())?;
        Ok(revoked_at.is_some())
    }

    /// List all active (non-revoked, non-expired) grants for a user.
    pub async fn list_active_for_user<'a>(
        user_id: i32,
        executor: &mut UnifiedExecutor<'a>,
    ) -> Result<Vec<GrantEntity>, sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let statement = Query::select()
            .from(GRANTS_TABLE)
            .columns([
                (GRANTS_TABLE, GrantIden::Id),
                (GRANTS_TABLE, GrantIden::GrantId),
                (GRANTS_TABLE, GrantIden::User),
                (GRANTS_TABLE, GrantIden::ClientId),
                (GRANTS_TABLE, GrantIden::ClientCnfKey),
                (GRANTS_TABLE, GrantIden::Capabilities),
                (GRANTS_TABLE, GrantIden::IssuedAt),
                (GRANTS_TABLE, GrantIden::ExpiresAt),
                (GRANTS_TABLE, GrantIden::RevokedAt),
                (GRANTS_TABLE, GrantIden::CreatedAt),
            ])
            .column((USER_TABLE, UserIden::PublicKey))
            .left_join(
                USER_TABLE,
                Expr::col((GRANTS_TABLE, GrantIden::User))
                    .eq(Expr::col((USER_TABLE, UserIden::Id))),
            )
            .and_where(Expr::col((GRANTS_TABLE, GrantIden::User)).eq(user_id))
            .and_where(Expr::col((GRANTS_TABLE, GrantIden::RevokedAt)).is_null())
            .and_where(Expr::col((GRANTS_TABLE, GrantIden::ExpiresAt)).gt(now))
            .to_owned();

        let (query, values) = statement.build_sqlx(PostgresQueryBuilder);
        let con = executor.get_con().await?;
        sqlx::query_as_with(&query, values).fetch_all(con).await
    }
}

/// Data needed to create a new grant.
pub struct NewGrant {
    pub grant_id: GrantId,
    pub user_id: i32,
    pub client_id: ClientId,
    pub client_cnf_key: String,
    pub capabilities: Capabilities,
    pub issued_at: u64,
    pub expires_at: u64,
}

/// A grant entity as stored in the database.
#[derive(Debug, Clone)]
pub struct GrantEntity {
    pub id: i32,
    pub grant_id: GrantId,
    pub user_id: i32,
    pub user_pubkey: PublicKey,
    pub client_id: ClientId,
    pub client_cnf_key: String,
    pub capabilities: Capabilities,
    pub issued_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub created_at: sqlx::types::chrono::NaiveDateTime,
}

impl FromRow<'_, PgRow> for GrantEntity {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let (id, grant_id, user_id, user_pubkey, client_id, client_cnf_key) =
            parse_identity_fields(row)?;
        let (capabilities, issued_at, expires_at, revoked_at, created_at) =
            parse_temporal_fields(row)?;
        Ok(GrantEntity {
            id, grant_id, user_id, user_pubkey, client_id, client_cnf_key,
            capabilities, issued_at, expires_at, revoked_at, created_at,
        })
    }
}

fn parse_identity_fields(
    row: &PgRow,
) -> Result<(i32, GrantId, i32, PublicKey, ClientId, String), sqlx::Error> {
    let id: i32 = row.try_get(GrantIden::Id.to_string().as_str())?;
    let grant_id: String = row.try_get(GrantIden::GrantId.to_string().as_str())?;
    let grant_id = GrantId::parse(&grant_id).map_err(|e| sqlx::Error::Decode(e.into()))?;
    let user_id: i32 = row.try_get(GrantIden::User.to_string().as_str())?;
    let user_pubkey: String = row.try_get(UserIden::PublicKey.to_string().as_str())?;
    let user_pubkey: PublicKey = user_pubkey
        .try_into()
        .map_err(|e: pkarr::errors::PublicKeyError| sqlx::Error::Decode(e.into()))?;
    let client_id: String = row.try_get(GrantIden::ClientId.to_string().as_str())?;
    let client_id = ClientId::new(&client_id).map_err(|e| sqlx::Error::Decode(e.into()))?;
    let client_cnf_key: String = row.try_get(GrantIden::ClientCnfKey.to_string().as_str())?;
    Ok((id, grant_id, user_id, user_pubkey, client_id, client_cnf_key))
}

fn parse_temporal_fields(
    row: &PgRow,
) -> Result<(Capabilities, i64, i64, Option<i64>, sqlx::types::chrono::NaiveDateTime), sqlx::Error> {
    let capabilities: String = row.try_get(GrantIden::Capabilities.to_string().as_str())?;
    let capabilities: Capabilities = capabilities
        .as_str()
        .try_into()
        .map_err(|e: pubky_common::capabilities::Error| sqlx::Error::Decode(e.into()))?;
    let issued_at: i64 = row.try_get(GrantIden::IssuedAt.to_string().as_str())?;
    let expires_at: i64 = row.try_get(GrantIden::ExpiresAt.to_string().as_str())?;
    let revoked_at: Option<i64> = row.try_get(GrantIden::RevokedAt.to_string().as_str())?;
    let created_at = row.try_get(GrantIden::CreatedAt.to_string().as_str())?;
    Ok((capabilities, issued_at, expires_at, revoked_at, created_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pubky_common::{
        capabilities::{Capabilities, Capability},
        crypto::Keypair,
        auth::jws::{ClientId, GrantId},
    };

    use crate::persistence::sql::{entities::user::UserRepository, SqlDb};

    fn make_new_grant(user_id: i32) -> NewGrant {
        let now = chrono::Utc::now().timestamp() as u64;
        NewGrant {
            grant_id: GrantId::generate(),
            user_id,
            client_id: ClientId::new("test.app").unwrap(),
            client_cnf_key: Keypair::random().public_key().z32(),
            capabilities: Capabilities::builder().cap(Capability::root()).finish(),
            issued_at: now,
            expires_at: now + 3600,
        }
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_and_get_grant() {
        let db = SqlDb::test().await;
        let keypair = Keypair::random();
        let user = UserRepository::create(&keypair.public_key(), &mut db.pool().into())
            .await
            .unwrap();

        let new_grant = make_new_grant(user.id);
        let grant_id = new_grant.grant_id.clone();
        let client_id = new_grant.client_id.clone();
        let caps = new_grant.capabilities.clone();
        let issued_at = new_grant.issued_at;
        let expires_at = new_grant.expires_at;

        GrantRepository::create(&new_grant, &mut db.pool().into())
            .await
            .unwrap();

        let entity = GrantRepository::get_by_grant_id(&grant_id, &mut db.pool().into())
            .await
            .unwrap();

        assert_eq!(entity.grant_id, grant_id);
        assert_eq!(entity.user_id, user.id);
        assert_eq!(entity.user_pubkey, keypair.public_key());
        assert_eq!(entity.client_id, client_id);
        assert_eq!(entity.capabilities, caps);
        assert_eq!(entity.issued_at, issued_at as i64);
        assert_eq!(entity.expires_at, expires_at as i64);
        assert!(entity.revoked_at.is_none());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_create_grant_is_idempotent() {
        let db = SqlDb::test().await;
        let user = UserRepository::create(&Keypair::random().public_key(), &mut db.pool().into())
            .await
            .unwrap();

        let new_grant = make_new_grant(user.id);
        GrantRepository::create(&new_grant, &mut db.pool().into())
            .await
            .unwrap();
        // Second insert with same grant_id should succeed (ON CONFLICT DO NOTHING)
        GrantRepository::create(&new_grant, &mut db.pool().into())
            .await
            .unwrap();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_revoke_and_is_revoked() {
        let db = SqlDb::test().await;
        let user = UserRepository::create(&Keypair::random().public_key(), &mut db.pool().into())
            .await
            .unwrap();

        let new_grant = make_new_grant(user.id);
        let grant_id = new_grant.grant_id.clone();
        GrantRepository::create(&new_grant, &mut db.pool().into())
            .await
            .unwrap();

        assert!(!GrantRepository::is_revoked(&grant_id, &mut db.pool().into()).await.unwrap());

        GrantRepository::revoke(&grant_id, &mut db.pool().into())
            .await
            .unwrap();

        assert!(GrantRepository::is_revoked(&grant_id, &mut db.pool().into()).await.unwrap());

        let entity = GrantRepository::get_by_grant_id(&grant_id, &mut db.pool().into())
            .await
            .unwrap();
        assert!(entity.revoked_at.is_some());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_list_active_for_user() {
        let db = SqlDb::test().await;
        let user = UserRepository::create(&Keypair::random().public_key(), &mut db.pool().into())
            .await
            .unwrap();

        // Active grant
        let active = make_new_grant(user.id);
        let active_id = active.grant_id.clone();
        GrantRepository::create(&active, &mut db.pool().into())
            .await
            .unwrap();

        // Revoked grant
        let revoked = make_new_grant(user.id);
        let revoked_id = revoked.grant_id.clone();
        GrantRepository::create(&revoked, &mut db.pool().into())
            .await
            .unwrap();
        GrantRepository::revoke(&revoked_id, &mut db.pool().into())
            .await
            .unwrap();

        // Expired grant
        let now = chrono::Utc::now().timestamp() as u64;
        let mut expired = make_new_grant(user.id);
        expired.issued_at = now.saturating_sub(7200);
        expired.expires_at = now.saturating_sub(3600);
        GrantRepository::create(&expired, &mut db.pool().into())
            .await
            .unwrap();

        let list = GrantRepository::list_active_for_user(user.id, &mut db.pool().into())
            .await
            .unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].grant_id, active_id);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_list_active_for_user_empty() {
        let db = SqlDb::test().await;
        let user = UserRepository::create(&Keypair::random().public_key(), &mut db.pool().into())
            .await
            .unwrap();

        let list = GrantRepository::list_active_for_user(user.id, &mut db.pool().into())
            .await
            .unwrap();
        assert!(list.is_empty());
    }
}
